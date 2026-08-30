//! Interactive UART learn session (QinHeng, lock held, no DTR while listening).

use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::device::RealDevice;
use crate::learn_uart_impl::input::{self, SkipWatch};
use crate::learn_uart_impl::parse::{parse_line, Accumulator, ParsedLine};
use crate::learn_uart_impl::report::{
    assemble, default_report_path, now_stamp, publish_latest, uart_log_path, unattended_human,
    unique_report_path, Briefing, HumanStep, Report, ReportStamp, Status,
};
use crate::learn_uart_impl::stamp::utc_rfc3339_millis;
use crate::learn_uart_impl::steps::{
    catalog, expected_total_secs, format_session_briefing, line_matches, parse_human_label,
    parse_optional_note, parse_yes_no, select, Consent, StepId, StepSpec, TimeoutReply, WaitFor,
    UNATTENDED_SECS, UNKNOWN_LABEL,
};
use crate::learn_uart_impl::term;
use crate::original::Layout;
use crate::Error;

/// Session flags (plain data; CLIs own clap types).
pub struct LearnUartArgs {
    /// `--port` / `ESPFLASH_PORT`.
    pub port: Option<String>,
    /// Extra YAML copy. Canonical file always goes under `backups/original/<serial>/learn-uart/`.
    pub report: Option<PathBuf>,
    /// `--skip` tokens.
    pub skip: Vec<String>,
    /// `--only` tokens (empty means the full catalog).
    pub only: Vec<String>,
    /// Override per-step UART wait.
    pub step_timeout_secs: Option<u32>,
    /// Optional `flash-app` payload.
    pub image: Option<PathBuf>,
    /// Required with `--image` or `--restore-app0`.
    pub yes: bool,
    /// Restore factory `app0` after the session.
    pub restore_app0: bool,
    /// Heartbeat + boot lines only.
    pub unattended_only: bool,
}

/// Run the session. [`crate::learn_uart`] holds [`crate::uart_lock::UartSession`]
/// for the whole call; nested flash-app / restore-app0 do not take the lock again.
pub fn run(layout: &Layout, args: LearnUartArgs) -> Result<(), Error> {
    let port = crate::detect::resolve_sticky_port(args.port)?;
    crate::detect::require_sticky_ch343(&port)?;

    let original = crate::original::require_original_from_port(layout, &port)?;
    let factory_serial = original.manifest.factory_serial.clone();
    anstream::eprintln!(
        "learn-uart: bound to backups/original/<factory-serial>/ (report in learn-uart/)"
    );

    if args.image.is_some() && !args.yes {
        return Err(Error::FlashNotConfirmed);
    }
    if args.restore_app0 && !args.yes {
        return Err(Error::RestoreNotConfirmed);
    }

    if let Some(image) = args.image.as_ref() {
        crate::flash_app_impl::flash_app(&RealDevice, layout, &port, image, args.yes)?;
        anstream::eprintln!(
            "learn-uart: flash-app finished; listening with the board sitting still"
        );
    }

    let skip_ids = parse_step_tokens(&args.skip, "--skip")?;
    let only_ids = parse_step_tokens(&args.only, "--only")?;
    if !only_ids.is_empty() && !args.unattended_only {
        let preview = select(Consent::ALL, &skip_ids, &only_ids, false);
        if preview.is_empty() {
            return Err(Error::Device(
                "no human steps left after --only / --skip; try touch, buttons, vbus, imu, sd"
                    .into(),
            ));
        }
    }
    let skipped_by_flag: Vec<String> = args.skip.clone();
    let only_tokens: Vec<String> = args.only.clone();

    let stdin_tty = io::stdin().is_terminal();
    if !args.unattended_only && !stdin_tty {
        return Err(Error::LearnNeedsTty);
    }

    let briefing = if args.unattended_only {
        Briefing {
            expected_minutes: expected_total_secs(&[], true).div_ceil(60),
            present_for_full_session: false,
            noisy_ok: false,
            microsd_handy: false,
            free_to_move: false,
            both_hands_free: false,
            terminal_in_view: false,
        }
    } else {
        interactive_briefing(&skip_ids, &only_ids)?
    };

    if !args.unattended_only && !briefing.present_for_full_session {
        anstream::eprintln!(
            "learn-uart: you are not free for the full window. Continuing with listen-only (no hands-on steps)."
        );
    }

    let unattended_only = args.unattended_only || !briefing.present_for_full_session;
    let steps = select(
        consent_from(&briefing),
        &skip_ids,
        &only_ids,
        unattended_only,
    );

    let stamp = now_stamp();
    let canonical = unique_report_path(default_report_path(layout, &factory_serial, &stamp));
    let mut uart_log = UartLog::create(uart_log_path(&canonical))?;

    say(
        &mut uart_log,
        format!("listening with the board sitting still ({UNATTENDED_SECS}s)"),
    )?;
    let mut uart = ListenUart::open(&port)?;

    let mut acc = Accumulator::default();
    let mut line_buf = Vec::new();

    let until = Instant::now() + Duration::from_secs(u64::from(UNATTENDED_SECS));
    {
        let mut ctx = ListenCtx {
            uart: &mut uart,
            log: &mut uart_log,
            line_buf: &mut line_buf,
            acc: &mut acc,
        };
        drain_until(&mut ctx, until, |_| false, None)?;
    }

    if let Some((hash, dirty)) = &acc.firmware_git {
        let host = crate::git::package_git();
        say(
            &mut uart_log,
            format!("firmware git={hash} dirty={}", u8::from(*dirty)),
        )?;
        if hash.as_str() != host.hash || *dirty != host.dirty {
            say(
                &mut uart_log,
                format!(
                    "host git={} dirty={} (image and host differ; rebuild the operator image if you meant this tree)",
                    host.hash,
                    u8::from(host.dirty)
                ),
            )?;
        }
    } else {
        say(
            &mut uart_log,
            "no firmware git= line (rebuild simple-debug with its build.rs stamp)",
        )?;
    }

    let mut human = if unattended_only {
        unattended_human()
    } else {
        Default::default()
    };

    if !unattended_only {
        let imu0 = acc.heartbeat.as_ref().map(|h| h.imu.clone());
        let n_steps = steps.len();
        for (index, step) in steps.iter().enumerate() {
            let timeout = args.step_timeout_secs.unwrap_or(step.timeout_secs);
            let mut ctx = ListenCtx {
                uart: &mut uart,
                log: &mut uart_log,
                line_buf: &mut line_buf,
                acc: &mut acc,
            };
            let outcome =
                run_human_step(&mut ctx, step, index + 1, n_steps, timeout, imu0.as_deref())?;
            human.insert(step.yaml_key.to_string(), outcome);
        }
        for spec in catalog() {
            human.entry(spec.yaml_key.to_string()).or_insert({
                let (human_label, operator_note) = if spec.button.is_some() {
                    (Some(UNKNOWN_LABEL.into()), None)
                } else {
                    (None, None)
                };
                HumanStep {
                    status: Status::Skipped,
                    operator_says_tried: None,
                    skip_reason: Some(skip_reason(
                        spec,
                        consent_from(&briefing),
                        &skip_ids,
                        &only_ids,
                    )),
                    notes: None,
                    human_label,
                    operator_note,
                }
            });
        }
    }

    let report = assemble(
        &acc,
        briefing,
        human,
        ReportStamp {
            factory_serial: factory_serial.clone(),
            captured_at: stamp.clone(),
            unattended_only,
            skipped_by_flag,
            only: only_tokens,
            complete: true,
            package_git: crate::git::package_git(),
        },
    );
    write_report_file(&report, &canonical)?;
    say(&mut uart_log, format!("wrote YAML {}", canonical.display()))?;
    if let Some(extra) = args.report.as_ref() {
        if extra != &canonical {
            write_report_file(&report, extra)?;
        }
    }

    let restore_port = uart.path.clone();
    if args.restore_app0 {
        let restore = if io::stdin().is_terminal() {
            confirm_factory_restore(&mut uart_log)?
        } else {
            true
        };
        drop(uart);
        if restore {
            say_go(
                &mut uart_log,
                "Writing factory software now. Put the board down. Do not unplug.",
            )?;
            thread::sleep(Duration::from_millis(500));
            crate::restore_impl::restore(&RealDevice, layout, &restore_port, true, Some("app0"))?;
            say_ok(&mut uart_log, "Factory software is back.")?;
        } else {
            say(
                &mut uart_log,
                "Keeping the current image. Factory software was not restored.",
            )?;
        }
    } else {
        drop(uart);
    }

    drop(uart_log);
    publish_latest(&canonical)?;
    anstream::eprintln!(
        "learn-uart: latest {}",
        canonical
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(crate::learn_uart_impl::report::LATEST_YAML_NAME)
            .display()
    );

    Ok(())
}

struct UartLog {
    file: fs::File,
}

impl UartLog {
    fn create(path: PathBuf) -> Result<Self, Error> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let file = fs::File::create(&path)?;
        anstream::eprintln!("learn-uart: living UART log {}", path.display());
        Ok(Self { file })
    }

    fn write_line(&mut self, kind: &str, text: &str) -> Result<(), Error> {
        let ts = utc_rfc3339_millis();
        writeln!(self.file, "{ts} {kind} {text}")?;
        self.file.flush()?;
        Ok(())
    }
}

fn say(log: &mut UartLog, msg: impl AsRef<str>) -> Result<(), Error> {
    say_styled(log, None, msg)
}

fn say_ok(log: &mut UartLog, msg: impl AsRef<str>) -> Result<(), Error> {
    say_styled(log, Some(term::ok), msg)
}

fn say_go(log: &mut UartLog, msg: impl AsRef<str>) -> Result<(), Error> {
    say_styled(log, Some(term::go), msg)
}

fn say_bad(log: &mut UartLog, msg: impl AsRef<str>) -> Result<(), Error> {
    say_styled(log, Some(term::bad), msg)
}

fn say_styled(
    log: &mut UartLog,
    style: Option<fn(&str) -> String>,
    msg: impl AsRef<str>,
) -> Result<(), Error> {
    let msg = msg.as_ref();
    let ts = utc_rfc3339_millis();
    let body = match style {
        Some(style) => style(msg),
        None => msg.to_string(),
    };
    anstream::eprintln!("{} learn-uart: {body}", term::dim(&format!("[{ts}]")));
    log.write_line("host", msg)
}

fn consent_from(briefing: &Briefing) -> Consent {
    Consent {
        noisy_ok: briefing.noisy_ok,
        microsd_handy: briefing.microsd_handy,
        free_to_move: briefing.free_to_move,
        both_hands_free: briefing.both_hands_free,
    }
}

fn skip_reason(spec: &StepSpec, consent: Consent, skip: &[StepId], only: &[StepId]) -> String {
    if !only.is_empty() && !only.contains(&spec.id) {
        return "not_in_only".into();
    }
    if skip.contains(&spec.id) {
        return "skip_flag".into();
    }
    if spec.noisy && !consent.noisy_ok {
        return "noisy_not_ok".into();
    }
    if spec.needs_microsd && !consent.microsd_handy {
        return "no_microsd".into();
    }
    if spec.needs_free_motion && !consent.free_to_move {
        return "cannot_move_freely".into();
    }
    if spec.needs_both_hands && !consent.both_hands_free {
        return "hands_not_free".into();
    }
    "not_selected".into()
}

fn parse_step_tokens(tokens: &[String], flag: &str) -> Result<Vec<StepId>, Error> {
    let mut out = Vec::new();
    for token in tokens {
        let ids = StepId::from_skip_token(token).ok_or_else(|| {
            Error::Device(format!(
                "unknown {flag} {token:?}; try buttons, vbus, imu, sd_detect, touch"
            ))
        })?;
        out.extend(ids);
    }
    Ok(out)
}

fn interactive_briefing(skip: &[StepId], only: &[StepId]) -> Result<Briefing, Error> {
    let preview = select(Consent::ALL, skip, only, false);
    let upper_secs = expected_total_secs(&preview, false);
    let upper_minutes = upper_secs.div_ceil(60);
    anstream::eprint!("{}", format_session_briefing(&preview));
    anstream::eprintln!();

    let present = ask_yn(
        &format!("Can you be present for the full ~{upper_minutes} minute(s)?"),
        true,
    )?;
    if !present {
        return Ok(Briefing {
            expected_minutes: expected_total_secs(&[], true).div_ceil(60),
            present_for_full_session: false,
            noisy_ok: false,
            microsd_handy: false,
            free_to_move: false,
            both_hands_free: false,
            terminal_in_view: false,
        });
    }

    let need_both_hands = preview.iter().any(|s| s.needs_both_hands);
    let need_move = preview.iter().any(|s| s.needs_free_motion);
    let need_noisy = preview.iter().any(|s| s.noisy);
    let need_sd = preview.iter().any(|s| s.needs_microsd);

    anstream::eprintln!();
    let both_hands = if need_both_hands {
        ask_yn(
            "Will both hands be free to handle the board? (Desk button/touch steps can still run if not.)",
            true,
        )?
    } else {
        true
    };
    let free_to_move = if need_move {
        ask_yn(
            "Can you lift and rotate the board? (A short USB cable often cannot follow it.)",
            true,
        )?
    } else {
        true
    };
    let terminal_in_view = ask_yn("Can you see this terminal while holding the board?", true)?;
    let noisy = if need_noisy {
        ask_yn(
            "Okay to unplug USB-C? (This computer will go quiet until you plug the same cable back in.)",
            true,
        )?
    } else {
        true
    };
    let microsd = if need_sd {
        ask_yn(
            "Do you have a working MicroSD card handy? (We will not mount or format it.)",
            false,
        )?
    } else {
        false
    };

    let consent = Consent {
        noisy_ok: noisy,
        microsd_handy: microsd,
        free_to_move,
        both_hands_free: both_hands,
    };
    let remaining = select(consent, skip, only, false);
    let secs = expected_total_secs(&remaining, false);
    let minutes = secs.div_ceil(60);
    anstream::eprintln!();
    anstream::eprintln!(
        "learn-uart: {n} step(s), about {minutes} minute(s).",
        n = remaining.len()
    );
    for step in &remaining {
        anstream::eprintln!("  - {}", step.title);
    }
    anstream::eprintln!();
    if !terminal_in_view {
        anstream::eprintln!(
            "You will put the board down to look at this terminal. We pause after each wait so you can type."
        );
    }
    if !free_to_move {
        anstream::eprintln!("Skipping tilt (not enough cable slack).");
    }
    if !both_hands {
        anstream::eprintln!(
            "Skipping two-handed steps (USB unplug, tilt, MicroSD). Buttons and touch can stay on the desk."
        );
    }

    Ok(Briefing {
        expected_minutes: minutes,
        present_for_full_session: true,
        noisy_ok: noisy,
        microsd_handy: microsd,
        free_to_move,
        both_hands_free: both_hands,
        terminal_in_view,
    })
}

fn ask_yn(prompt: &str, default_yes: bool) -> Result<bool, Error> {
    loop {
        anstream::eprint!("{prompt} {} ", term::yn_hint(default_yes));
        let _ = io::stderr().flush();
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        if line.trim().is_empty() {
            return Ok(default_yes);
        }
        if let Some(answer) = parse_yes_no(&line) {
            return Ok(answer);
        }
        anstream::eprintln!("Please answer y or n, or press Enter for the default.");
    }
}

struct ListenCtx<'a> {
    uart: &'a mut ListenUart,
    log: &'a mut UartLog,
    line_buf: &'a mut Vec<u8>,
    acc: &'a mut Accumulator,
}

fn run_human_step(
    ctx: &mut ListenCtx<'_>,
    step: &StepSpec,
    index: usize,
    total: usize,
    timeout_secs: u32,
    imu_baseline: Option<&str>,
) -> Result<HumanStep, Error> {
    loop {
        print_step_card(step, index, total, timeout_secs);
        ctx.log.write_line("host", &step.instruction())?;

        if step.snapshot_before_wait {
            anstream::eprintln!();
            anstream::eprintln!("{}", term::topic("Now"));
            anstream::eprintln!("  Set the board still on the desk (glass up is fine).");
            anstream::eprintln!();
            anstream::eprintln!(
                "  {} sitting still     {}",
                term::key("Enter"),
                term::skip_hint()
            );
            anstream::eprintln!("{}", term::skip_no_enter());
            anstream::eprintln!();
            ctx.log.write_line(
                "host",
                "Set the board still on the desk. Press Enter when it is sitting.",
            )?;
            if !input::wait_enter_or_decline(false)? {
                return Ok(operator_skip(step));
            }
            say(ctx.log, "Waiting a few seconds for the board to sit still…")?;
            let sample_until = Instant::now() + Duration::from_secs(3);
            let sample = drain_until(ctx, sample_until, |_| false, None)?;
            if matches!(sample, DrainEnd::Skip) {
                return Ok(operator_skip(step));
            }
            match step.id {
                StepId::Imu => {
                    let pose = ctx
                        .acc
                        .heartbeat
                        .as_ref()
                        .map(|h| h.imu.as_str())
                        .unwrap_or("unknown");
                    ctx.log
                        .write_line("host", &format!("baseline imu={pose}"))?;
                    say_ok(
                        ctx.log,
                        "READY. Lift or rotate it and hold still for about a second.",
                    )?;
                }
                _ => {
                    say_ok(ctx.log, "READY.")?;
                }
            }
        }

        if step.ready_before_wait {
            anstream::eprintln!();
            anstream::eprintln!("{}", term::topic("Now"));
            match step.id {
                StepId::Gt911Contacts => {
                    anstream::eprintln!("  Press Enter when you are ready.");
                    anstream::eprintln!("  Do not touch the glass yet.");
                }
                StepId::SdDetect => {
                    anstream::eprintln!("  Have the card in hand.");
                    anstream::eprintln!("  Press Enter when you are ready to insert or remove it.");
                }
                _ => {
                    anstream::eprintln!("  Press Enter when you are ready.");
                }
            }
            anstream::eprintln!();
            anstream::eprintln!("  {} ready     {}", term::key("Enter"), term::skip_hint());
            anstream::eprintln!("{}", term::skip_no_enter());
            anstream::eprintln!();
            ctx.log
                .write_line("host", "waiting for Enter before the timed wait")?;
            if !input::wait_enter_or_decline(false)? {
                return Ok(operator_skip(step));
            }
            match step.id {
                StepId::Gt911Contacts => {
                    say_ok(ctx.log, "Put a finger on the glass now.")?;
                }
                StepId::SdDetect => {
                    say_ok(ctx.log, "Insert or remove the card now.")?;
                }
                _ => {
                    say_ok(ctx.log, "Go.")?;
                }
            }
        }

        let imu_now = ctx.acc.heartbeat.as_ref().map(|h| h.imu.clone());
        let baseline = imu_now.as_deref().or(imu_baseline);
        let until = Instant::now() + Duration::from_secs(u64::from(timeout_secs));
        let wait = drain_until(
            ctx,
            until,
            |line| line_matches(step.wait, line, baseline),
            Some(step.wait),
        )?;
        match wait {
            DrainEnd::Matched { note } => {
                let (human_label, operator_note) = ask_button_map(step)?;
                return Ok(HumanStep {
                    status: Status::Observed,
                    operator_says_tried: Some(true),
                    skip_reason: None,
                    notes: note,
                    human_label,
                    operator_note,
                });
            }
            DrainEnd::Skip => {
                return Ok(operator_skip(step));
            }
            DrainEnd::Timeout if step.timeout_is_success => {
                say_ok(
                    ctx.log,
                    "Nothing extra showed up while you moved it. That's a valid result.",
                )?;
                return Ok(HumanStep {
                    status: Status::Observed,
                    operator_says_tried: Some(true),
                    skip_reason: None,
                    notes: Some("no_edge_within_timeout".into()),
                    human_label: None,
                    operator_note: None,
                });
            }
            DrainEnd::Timeout => {
                anstream::eprintln!();
                if ctx.acc.gt911_poll_failed && matches!(step.wait, WaitFor::ContactsNonZero) {
                    say_bad(
                        ctx.log,
                        "The glass did not report a finger. If you did touch it, that is still useful.",
                    )?;
                } else {
                    say_bad(
                        ctx.log,
                        format!("We didn't see a response (waited {timeout_secs}s)."),
                    )?;
                }
                anstream::eprintln!();
                anstream::eprint!(
                    "Did you try this?  {} tried  {} did not  {} retry  ",
                    term::key("y"),
                    term::key("n"),
                    term::key("r")
                );
                anstream::eprintln!();
                anstream::eprintln!("{}", term::dim("Press y, n, or r. You do not need Enter."));
                let _ = io::stderr().flush();
                match input::wait_timeout_reply()? {
                    TimeoutReply::Retry => continue,
                    TimeoutReply::Tried => {
                        let (human_label, operator_note) = ask_button_map(step)?;
                        let notes = if matches!(step.wait, WaitFor::ContactsNonZero) {
                            if ctx.acc.gt911_poll_failed {
                                "uart_timeout_operator_says_tried; gt911_poll_failed".into()
                            } else {
                                format!(
                                    "uart_timeout_operator_says_tried; gt911_st_max={:#04x}; gt911_int={}",
                                    ctx.acc.gt911_status_max,
                                    match ctx.acc.gt911_int {
                                        Some(true) => "1",
                                        Some(false) => "0",
                                        None => "none",
                                    }
                                )
                            }
                        } else {
                            "uart_timeout_operator_says_tried".into()
                        };
                        return Ok(HumanStep {
                            status: Status::Timeout,
                            operator_says_tried: Some(true),
                            skip_reason: None,
                            notes: Some(notes),
                            human_label,
                            operator_note,
                        });
                    }
                    TimeoutReply::DidNotTry => {
                        return Ok(HumanStep {
                            status: Status::Skipped,
                            operator_says_tried: Some(false),
                            skip_reason: Some("timeout_not_tried".into()),
                            notes: None,
                            human_label: step.button.map(|_| UNKNOWN_LABEL.into()),
                            operator_note: None,
                        });
                    }
                }
            }
        }
    }
}

fn operator_skip(step: &StepSpec) -> HumanStep {
    HumanStep {
        status: Status::Skipped,
        operator_says_tried: Some(false),
        skip_reason: Some("operator_skip".into()),
        notes: None,
        human_label: step.button.map(|_| UNKNOWN_LABEL.into()),
        operator_note: None,
    }
}

fn confirm_factory_restore(log: &mut UartLog) -> Result<bool, Error> {
    anstream::eprintln!();
    anstream::eprintln!("{}", term::rule());
    anstream::eprintln!("{}", term::go("ABOUT TO WRITE FACTORY SOFTWARE"));
    anstream::eprintln!();
    anstream::eprintln!("{}", term::topic("Now"));
    anstream::eprintln!("  Put the board down. Do not hold it.");
    anstream::eprintln!("  USB-C must stay still. A jiggle can interrupt the write.");
    anstream::eprintln!();
    anstream::eprintln!(
        "{}",
        term::dim("This puts the original app back. It takes about a minute. Do not unplug.")
    );
    anstream::eprintln!();
    anstream::eprintln!(
        "  {} board is sitting still     {} keep this image",
        term::key("Enter"),
        term::key("n")
    );
    anstream::eprintln!(
        "{}",
        term::dim("Press n to keep this image. You do not need Enter. Enter starts writing.")
    );
    anstream::eprintln!();
    log.write_line("host", "waiting for Enter before factory restore")?;
    input::wait_enter_or_decline(true)
}

fn print_step_card(step: &StepSpec, index: usize, total: usize, timeout_secs: u32) {
    anstream::eprintln!();
    anstream::eprintln!("{}", term::rule());
    anstream::eprintln!("{}", term::step_title(index, total, step.title));
    anstream::eprintln!();
    anstream::eprintln!("{}", term::topic("Do"));
    for line in step.do_lines {
        anstream::eprintln!("  {line}");
    }
    anstream::eprintln!();
    if let Some(note) = step.capture_note {
        anstream::eprintln!("{} {note}", term::topic("Recording"));
        anstream::eprintln!();
    }
    if let Some(note) = step.attention_note {
        anstream::eprintln!("{}", term::dim(note));
        anstream::eprintln!();
    }
    anstream::eprintln!(
        "{}",
        term::dim(&format!(
            "Usually ~{}s. We wait up to {}s.",
            step.expected_secs, timeout_secs
        ))
    );
    anstream::eprintln!();
    anstream::eprintln!("  {}", term::skip_hint());
    anstream::eprintln!("{}", term::skip_no_enter());
    anstream::eprintln!();
}

enum DrainEnd {
    Matched { note: Option<String> },
    Timeout,
    Skip,
}

/// USB-C is the CH343. Unplug looks like a host I/O error, not a firmware panic.
fn uart_unplugged(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::NotFound
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
    ) || matches!(error.raw_os_error(), Some(5 | 6 | 19 | 32))
}

/// GPIO9 unplug also unplugs the CH343, so the firmware `vbus` line is usually lost.
fn host_reconnect_closes(wait: Option<WaitFor>) -> bool {
    matches!(wait, Some(WaitFor::LevelEdge("vbus")))
}

struct ListenUart {
    path: String,
    serial: crate::cdc_listen::CdcListen,
}

impl ListenUart {
    fn open(path: &str) -> Result<Self, Error> {
        Ok(Self {
            path: path.to_string(),
            serial: crate::cdc_listen::CdcListen::open(path)?,
        })
    }

    fn reopen_until(
        &mut self,
        log: &mut UartLog,
        until: Instant,
        line_buf: &mut Vec<u8>,
    ) -> Result<bool, Error> {
        say_go(
            log,
            "USB-C is unplugged (expected). Plug the same cable back in.",
        )?;
        line_buf.clear();
        while Instant::now() < until {
            thread::sleep(Duration::from_millis(200));
            match try_open_replugged(&self.path) {
                Ok(Some(next)) => {
                    say_ok(log, "USB-C is back. This step is done.")?;
                    *self = next;
                    return Ok(true);
                }
                Ok(None) => {}
                Err(error) => return Err(error),
            }
        }
        say_bad(log, "USB-C did not come back before we stopped waiting.")?;
        Ok(false)
    }
}

fn try_open_replugged(preferred: &str) -> Result<Option<ListenUart>, Error> {
    let path = if Path::new(preferred).exists() {
        preferred.to_string()
    } else {
        match crate::detect::resolve_sticky_port(None) {
            Ok(path) => path,
            Err(Error::MissingStickyUart | Error::UnclassifiedUsbPort) => return Ok(None),
            Err(error) => return Err(error),
        }
    };
    match ListenUart::open(&path) {
        Ok(uart) => Ok(Some(uart)),
        Err(Error::Device(_)) | Err(Error::NotStickyUart { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

fn drain_until(
    ctx: &mut ListenCtx<'_>,
    until: Instant,
    mut pred: impl FnMut(&ParsedLine) -> bool,
    wait: Option<WaitFor>,
) -> Result<DrainEnd, Error> {
    let skip = SkipWatch::enter();
    let mut chunk = [0u8; 1024];
    while Instant::now() < until {
        if skip.poll_skip() {
            return Ok(DrainEnd::Skip);
        }
        match ctx.uart.serial.read(&mut chunk) {
            Ok(0) => {}
            Ok(n) => {
                ctx.line_buf.extend_from_slice(&chunk[..n]);
                if let Some(end) = consume_lines(ctx.line_buf, ctx.log, ctx.acc, &mut pred, wait)? {
                    return Ok(end);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::TimedOut => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if uart_unplugged(&error) => {
                if !ctx.uart.reopen_until(ctx.log, until, ctx.line_buf)? {
                    return Ok(DrainEnd::Timeout);
                }
                if host_reconnect_closes(wait) {
                    return Ok(DrainEnd::Matched {
                        note: Some("host: CH343 UART dropped with the cable and returned".into()),
                    });
                }
            }
            Err(error) => return Err(Error::Device(format!("UART read failed: {error}"))),
        }
    }
    let _ = consume_lines(ctx.line_buf, ctx.log, ctx.acc, &mut pred, wait)?;
    Ok(DrainEnd::Timeout)
}

fn consume_lines(
    line_buf: &mut Vec<u8>,
    log: &mut UartLog,
    acc: &mut Accumulator,
    pred: &mut impl FnMut(&ParsedLine) -> bool,
    wait: Option<WaitFor>,
) -> Result<Option<DrainEnd>, Error> {
    while let Some(pos) = line_buf.iter().position(|b| *b == b'\n') {
        let raw = line_buf.drain(..=pos).collect::<Vec<_>>();
        let text = String::from_utf8_lossy(&raw);
        let trimmed = text.trim_end();
        if !trimmed.is_empty() {
            log.write_line("device", trimmed)?;
        }
        let parsed = parse_line(trimmed);
        acc.observe(&parsed);
        if pred(&parsed) {
            let note = match (&parsed, wait) {
                (ParsedLine::Heartbeat(hb), Some(WaitFor::ImuChange)) => {
                    Some(format!("imu={}", hb.imu))
                }
                (ParsedLine::Contacts(n), Some(WaitFor::ContactsNonZero)) => {
                    Some(format!("contacts={n}"))
                }
                (ParsedLine::Level { from, to, .. }, _) => Some(format!("{from} -> {to}")),
                _ => None,
            };
            return Ok(Some(DrainEnd::Matched { note }));
        }
    }
    Ok(None)
}

fn ask_button_map(step: &StepSpec) -> Result<(Option<String>, Option<String>), Error> {
    let Some(button) = step.button else {
        return Ok((None, None));
    };
    anstream::eprintln!();
    anstream::eprintln!("{}", term::topic("What would you call this key"));
    anstream::eprintln!();
    anstream::eprintln!("  {}", button.enclosure_hint);
    anstream::eprintln!();
    anstream::eprint!("  {} unknown  ", term::key("Enter"));
    let _ = io::stderr().flush();
    let human_label = parse_human_label(&input::read_line()?);
    anstream::eprintln!();
    anstream::eprintln!("{}", term::topic("Short note if still unsure"));
    anstream::eprintln!();
    anstream::eprint!("  {} skip  ", term::key("Enter"));
    let _ = io::stderr().flush();
    let operator_note = parse_optional_note(&input::read_line()?);
    Ok((Some(human_label), operator_note))
}

fn write_report_file(report: &Report, path: &Path) -> Result<(), Error> {
    let yaml = noyalib::to_string(report).map_err(|error| Error::Yaml(error.to_string()))?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path, yaml.as_bytes())?;
    anstream::eprintln!("learn-uart: wrote {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::uart_unplugged;
    use std::io::{self, ErrorKind};

    #[test]
    fn broken_pipe_is_usb_unplug_not_a_session_crash() {
        assert!(uart_unplugged(&io::Error::new(
            ErrorKind::BrokenPipe,
            "Broken pipe"
        )));
        assert!(uart_unplugged(&io::Error::from_raw_os_error(5)));
        assert!(!uart_unplugged(&io::Error::new(
            ErrorKind::TimedOut,
            "timeout"
        )));
    }

    #[test]
    fn usb_replug_closes_only_the_vbus_wait() {
        use crate::learn_uart_impl::steps::WaitFor;
        assert!(super::host_reconnect_closes(Some(WaitFor::LevelEdge(
            "vbus"
        ))));
        assert!(!super::host_reconnect_closes(Some(WaitFor::LevelEdge(
            "sd_cd"
        ))));
        assert!(!super::host_reconnect_closes(Some(WaitFor::ButtonDown(4))));
        assert!(!super::host_reconnect_closes(None));
    }
}
