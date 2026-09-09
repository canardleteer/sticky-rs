///Shorthand for `OwnedView<ConnectRequestView<'static>>`.
pub type OwnedConnectRequestView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::ConnectRequestView<'static>,
>;
///Shorthand for `OwnedView<ConnectResponseView<'static>>`.
pub type OwnedConnectResponseView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::ConnectResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<StatusRequestView<'static>>`.
pub type OwnedStatusRequestView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::StatusRequestView<'static>,
>;
///Shorthand for `OwnedView<StatusResponseView<'static>>`.
pub type OwnedStatusResponseView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::StatusResponseView<'static>,
>;
///Shorthand for `OwnedView<ListTargetsRequestView<'static>>`.
pub type OwnedListTargetsRequestView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::ListTargetsRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<ListTargetsResponseView<'static>>`.
pub type OwnedListTargetsResponseView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::ListTargetsResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<InjectTouchRequestView<'static>>`.
pub type OwnedInjectTouchRequestView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::InjectTouchRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<InjectTouchResponseView<'static>>`.
pub type OwnedInjectTouchResponseView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::InjectTouchResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<InjectButtonRequestView<'static>>`.
pub type OwnedInjectButtonRequestView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::InjectButtonRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<InjectButtonResponseView<'static>>`.
pub type OwnedInjectButtonResponseView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::InjectButtonResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<GetSnapshotRequestView<'static>>`.
pub type OwnedGetSnapshotRequestView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::GetSnapshotRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<GetSnapshotResponseView<'static>>`.
pub type OwnedGetSnapshotResponseView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::GetSnapshotResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<SnapshotAckRequestView<'static>>`.
pub type OwnedSnapshotAckRequestView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotAckRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<SnapshotAckResponseView<'static>>`.
pub type OwnedSnapshotAckResponseView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotAckResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<SnapshotClearRequestView<'static>>`.
pub type OwnedSnapshotClearRequestView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotClearRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<SnapshotClearResponseView<'static>>`.
pub type OwnedSnapshotClearResponseView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotClearResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<RebootRequestView<'static>>`.
pub type OwnedRebootRequestView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::RebootRequestView<'static>,
>;
///Shorthand for `OwnedView<RebootResponseView<'static>>`.
pub type OwnedRebootResponseView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::RebootResponseView<'static>,
>;
///Shorthand for `OwnedView<DisconnectRequestView<'static>>`.
pub type OwnedDisconnectRequestView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::DisconnectRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<DisconnectResponseView<'static>>`.
pub type OwnedDisconnectResponseView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::DisconnectResponseView<
        'static,
    >,
>;
///Shorthand for `OwnedView<ShutdownRequestView<'static>>`.
pub type OwnedShutdownRequestView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::ShutdownRequestView<
        'static,
    >,
>;
///Shorthand for `OwnedView<ShutdownResponseView<'static>>`.
pub type OwnedShutdownResponseView = ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::ShutdownResponseView<
        'static,
    >,
>;
impl ::connectrpc::Encodable<crate::proto::sticky::remote::control::v1::ConnectResponse>
for crate::proto::sticky::remote::control::v1::__buffa::view::ConnectResponseView<'_> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<crate::proto::sticky::remote::control::v1::ConnectResponse>
for ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::ConnectResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
impl ::connectrpc::Encodable<crate::proto::sticky::remote::control::v1::StatusResponse>
for crate::proto::sticky::remote::control::v1::__buffa::view::StatusResponseView<'_> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<crate::proto::sticky::remote::control::v1::StatusResponse>
for ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::StatusResponseView<'static>,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
impl ::connectrpc::Encodable<
    crate::proto::sticky::remote::control::v1::ListTargetsResponse,
>
for crate::proto::sticky::remote::control::v1::__buffa::view::ListTargetsResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::proto::sticky::remote::control::v1::ListTargetsResponse,
>
for ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::ListTargetsResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
impl ::connectrpc::Encodable<
    crate::proto::sticky::remote::control::v1::InjectTouchResponse,
>
for crate::proto::sticky::remote::control::v1::__buffa::view::InjectTouchResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::proto::sticky::remote::control::v1::InjectTouchResponse,
>
for ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::InjectTouchResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
impl ::connectrpc::Encodable<
    crate::proto::sticky::remote::control::v1::InjectButtonResponse,
>
for crate::proto::sticky::remote::control::v1::__buffa::view::InjectButtonResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::proto::sticky::remote::control::v1::InjectButtonResponse,
>
for ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::InjectButtonResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
impl ::connectrpc::Encodable<
    crate::proto::sticky::remote::control::v1::GetSnapshotResponse,
>
for crate::proto::sticky::remote::control::v1::__buffa::view::GetSnapshotResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::proto::sticky::remote::control::v1::GetSnapshotResponse,
>
for ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::GetSnapshotResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
impl ::connectrpc::Encodable<
    crate::proto::sticky::remote::control::v1::SnapshotAckResponse,
>
for crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotAckResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::proto::sticky::remote::control::v1::SnapshotAckResponse,
>
for ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotAckResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
impl ::connectrpc::Encodable<
    crate::proto::sticky::remote::control::v1::SnapshotClearResponse,
>
for crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotClearResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::proto::sticky::remote::control::v1::SnapshotClearResponse,
>
for ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotClearResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
impl ::connectrpc::Encodable<crate::proto::sticky::remote::control::v1::RebootResponse>
for crate::proto::sticky::remote::control::v1::__buffa::view::RebootResponseView<'_> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<crate::proto::sticky::remote::control::v1::RebootResponse>
for ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::RebootResponseView<'static>,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
impl ::connectrpc::Encodable<
    crate::proto::sticky::remote::control::v1::DisconnectResponse,
>
for crate::proto::sticky::remote::control::v1::__buffa::view::DisconnectResponseView<
    '_,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<
    crate::proto::sticky::remote::control::v1::DisconnectResponse,
>
for ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::DisconnectResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
impl ::connectrpc::Encodable<crate::proto::sticky::remote::control::v1::ShutdownResponse>
for crate::proto::sticky::remote::control::v1::__buffa::view::ShutdownResponseView<'_> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<crate::proto::sticky::remote::control::v1::ShutdownResponse>
for ::buffa::view::OwnedView<
    crate::proto::sticky::remote::control::v1::__buffa::view::ShutdownResponseView<
        'static,
    >,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
/// Full service name for this service.
pub const REMOTE_DEBUG_CONTROL_SERVICE_SERVICE_NAME: &str = "sticky.remote.control.v1.RemoteDebugControlService";
/// Static [`Spec`](::connectrpc::Spec) for the `Connect` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const REMOTE_DEBUG_CONTROL_SERVICE_CONNECT_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/sticky.remote.control.v1.RemoteDebugControlService/Connect",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the `Status` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const REMOTE_DEBUG_CONTROL_SERVICE_STATUS_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/sticky.remote.control.v1.RemoteDebugControlService/Status",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the `ListTargets` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const REMOTE_DEBUG_CONTROL_SERVICE_LIST_TARGETS_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/sticky.remote.control.v1.RemoteDebugControlService/ListTargets",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the `InjectTouch` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const REMOTE_DEBUG_CONTROL_SERVICE_INJECT_TOUCH_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/sticky.remote.control.v1.RemoteDebugControlService/InjectTouch",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the `InjectButton` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const REMOTE_DEBUG_CONTROL_SERVICE_INJECT_BUTTON_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/sticky.remote.control.v1.RemoteDebugControlService/InjectButton",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the `GetSnapshot` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const REMOTE_DEBUG_CONTROL_SERVICE_GET_SNAPSHOT_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/sticky.remote.control.v1.RemoteDebugControlService/GetSnapshot",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the `SnapshotAck` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const REMOTE_DEBUG_CONTROL_SERVICE_SNAPSHOT_ACK_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/sticky.remote.control.v1.RemoteDebugControlService/SnapshotAck",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the `SnapshotClear` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const REMOTE_DEBUG_CONTROL_SERVICE_SNAPSHOT_CLEAR_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/sticky.remote.control.v1.RemoteDebugControlService/SnapshotClear",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the `Reboot` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const REMOTE_DEBUG_CONTROL_SERVICE_REBOOT_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/sticky.remote.control.v1.RemoteDebugControlService/Reboot",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the `Disconnect` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const REMOTE_DEBUG_CONTROL_SERVICE_DISCONNECT_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/sticky.remote.control.v1.RemoteDebugControlService/Disconnect",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the `Shutdown` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const REMOTE_DEBUG_CONTROL_SERVICE_SHUTDOWN_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/sticky.remote.control.v1.RemoteDebugControlService/Shutdown",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Unary RPCs that own GATT. BLE is central-out; a bidi stream does
/// not fit. Each method has its own request and response types.
///
/// # Implementing handlers
///
/// Implement methods with plain `async fn`; the returned future satisfies
/// the `Send` bound automatically.
///
/// **Unary and server-streaming requests** arrive as
/// [`ServiceRequest<'_, Req>`](::connectrpc::ServiceRequest): a zero-copy
/// view of the request plus its body, valid for the duration of the call.
/// Fields are read directly (`request.name` is a `&str` into the decoded
/// buffer) and the borrow may be held across `.await` points. Anything
/// that must outlive the call — `tokio::spawn`, channels, server state,
/// or data captured by a returned response stream — takes owned data:
/// call `request.to_owned_message()` (or copy the specific fields)
/// first.
///
/// **Client-streaming and bidi requests** arrive as
/// [`InboundStream<Req>`](::connectrpc::InboundStream) — a
/// `ServiceStream` of [`StreamMessage`](::connectrpc::StreamMessage)s.
/// Each item owns its decoded buffer and is `Send + 'static`, so items
/// can be buffered or moved into spawned tasks; read fields zero-copy
/// through the generated accessor methods (`item.name()`) or `.view()`,
/// convert with `.to_owned_message()`, or yield an item back unchanged —
/// `StreamMessage<M>` implements `Encodable<M>`.
///
/// Request types resolved through `extern_path` (e.g. well-known types
/// from another crate) use the same wrappers; the crate that owns the
/// type must be generated with buffa ≥ 0.9.0 and views enabled so the
/// backing `HasMessageView` impl exists.
///
/// The `impl Encodable<Out>` return bound accepts the owned `Out`, the
/// generated `OutView<'_>` / `OwnedOutView`,
/// [`MaybeBorrowed`](::connectrpc::MaybeBorrowed), or
/// [`PreEncoded`](::connectrpc::PreEncoded) for handlers that encode a
/// non-`'static` view internally and pass the bytes across the handler
/// boundary. View bodies are not emitted for output types mapped via
/// `extern_path` (the impl would be an orphan); return owned for
/// WKT/extern outputs.
///
/// Server-streaming and bidi-streaming methods return
/// `ServiceStream<impl Encodable<Out> + Send + use<Self>>`. The
/// `use<Self>` precise-capturing clause excludes `&self`'s lifetime and
/// the request's lifetime (unary methods use `use<'a, Self>` and may
/// borrow from `&self`), so stream items must be `'static` and cannot
/// borrow from the request. To stream view-encoded data, encode each
/// item inside the stream body and yield
/// [`PreEncoded`](::connectrpc::PreEncoded) — see its `# Streaming
/// example` doc.
#[allow(clippy::type_complexity)]
pub trait RemoteDebugControlService: Send + Sync + 'static {
    /// Start pair on `target`. Returns pairing immediately. Poll Status.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn connect<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::sticky::remote::control::v1::ConnectRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::sticky::remote::control::v1::ConnectResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Session meter for one advertise name.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn status<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::sticky::remote::control::v1::StatusRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::sticky::remote::control::v1::StatusResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Every target the owner currently tracks.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn list_targets<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::sticky::remote::control::v1::ListTargetsRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::sticky::remote::control::v1::ListTargetsResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Synthetic tap or slide on a connected target.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn inject_touch<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::sticky::remote::control::v1::InjectTouchRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::sticky::remote::control::v1::InjectTouchResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Synthetic product-key edge on a connected target.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn inject_button<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::sticky::remote::control::v1::InjectButtonRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::sticky::remote::control::v1::InjectButtonResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Arm LAST DRAW and return packed planes (caller writes files).
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn get_snapshot<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::sticky::remote::control::v1::GetSnapshotRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::sticky::remote::control::v1::GetSnapshotResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Release the armed snapshot nonce.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn snapshot_ack<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::sticky::remote::control::v1::SnapshotAckRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::sticky::remote::control::v1::SnapshotAckResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Operator abort (no nonce).
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn snapshot_clear<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::sticky::remote::control::v1::SnapshotClearRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::sticky::remote::control::v1::SnapshotClearResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Software-reset the embedded MCU (not this host).
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn reboot<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::sticky::remote::control::v1::RebootRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::sticky::remote::control::v1::RebootResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Drop GATT for one advertise name. Owner keeps listening.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn disconnect<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::sticky::remote::control::v1::DisconnectRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::sticky::remote::control::v1::DisconnectResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Stop the owner process after dropping every session.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn shutdown<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::sticky::remote::control::v1::ShutdownRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::sticky::remote::control::v1::ShutdownResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
}
/// Extension trait for registering a service implementation with a Router.
///
/// This trait is automatically implemented for all types that implement the service trait.
/// Prefer [`Router::add_service`](::connectrpc::Router::add_service) for
/// top-down registration; `register` remains available for compatibility
/// and cases where the service-first call shape is more convenient.
///
/// # Example
///
/// ```rust,ignore
/// use std::sync::Arc;
///
/// let service = Arc::new(MyServiceImpl);
/// let router = service.register(Router::new());
/// ```
pub trait RemoteDebugControlServiceExt: RemoteDebugControlService {
    /// Register this service implementation with a Router.
    ///
    /// Takes ownership of the `Arc<Self>` and returns a new Router with
    /// this service's methods registered.
    fn register(
        self: ::std::sync::Arc<Self>,
        router: ::connectrpc::Router,
    ) -> ::connectrpc::Router;
}
impl<S: RemoteDebugControlService> RemoteDebugControlServiceExt for S {
    fn register(
        self: ::std::sync::Arc<Self>,
        router: ::connectrpc::Router,
    ) -> ::connectrpc::Router {
        router
            .route_view(
                REMOTE_DEBUG_CONTROL_SERVICE_SERVICE_NAME,
                "Connect",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::sticky::remote::control::v1::__buffa::view::ConnectRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::sticky::remote::control::v1::ConnectRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.connect(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::sticky::remote::control::v1::ConnectResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_CONNECT_SPEC)
            .route_view(
                REMOTE_DEBUG_CONTROL_SERVICE_SERVICE_NAME,
                "Status",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::sticky::remote::control::v1::__buffa::view::StatusRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::sticky::remote::control::v1::StatusRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.status(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::sticky::remote::control::v1::StatusResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_STATUS_SPEC)
            .route_view(
                REMOTE_DEBUG_CONTROL_SERVICE_SERVICE_NAME,
                "ListTargets",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::sticky::remote::control::v1::__buffa::view::ListTargetsRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::sticky::remote::control::v1::ListTargetsRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.list_targets(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::sticky::remote::control::v1::ListTargetsResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_LIST_TARGETS_SPEC)
            .route_view(
                REMOTE_DEBUG_CONTROL_SERVICE_SERVICE_NAME,
                "InjectTouch",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::sticky::remote::control::v1::__buffa::view::InjectTouchRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::sticky::remote::control::v1::InjectTouchRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.inject_touch(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::sticky::remote::control::v1::InjectTouchResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_INJECT_TOUCH_SPEC)
            .route_view(
                REMOTE_DEBUG_CONTROL_SERVICE_SERVICE_NAME,
                "InjectButton",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::sticky::remote::control::v1::__buffa::view::InjectButtonRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::sticky::remote::control::v1::InjectButtonRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.inject_button(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::sticky::remote::control::v1::InjectButtonResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_INJECT_BUTTON_SPEC)
            .route_view(
                REMOTE_DEBUG_CONTROL_SERVICE_SERVICE_NAME,
                "GetSnapshot",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::sticky::remote::control::v1::__buffa::view::GetSnapshotRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::sticky::remote::control::v1::GetSnapshotRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.get_snapshot(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::sticky::remote::control::v1::GetSnapshotResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_GET_SNAPSHOT_SPEC)
            .route_view(
                REMOTE_DEBUG_CONTROL_SERVICE_SERVICE_NAME,
                "SnapshotAck",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotAckRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::sticky::remote::control::v1::SnapshotAckRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.snapshot_ack(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::sticky::remote::control::v1::SnapshotAckResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_SNAPSHOT_ACK_SPEC)
            .route_view(
                REMOTE_DEBUG_CONTROL_SERVICE_SERVICE_NAME,
                "SnapshotClear",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotClearRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::sticky::remote::control::v1::SnapshotClearRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.snapshot_clear(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::sticky::remote::control::v1::SnapshotClearResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_SNAPSHOT_CLEAR_SPEC)
            .route_view(
                REMOTE_DEBUG_CONTROL_SERVICE_SERVICE_NAME,
                "Reboot",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::sticky::remote::control::v1::__buffa::view::RebootRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::sticky::remote::control::v1::RebootRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.reboot(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::sticky::remote::control::v1::RebootResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_REBOOT_SPEC)
            .route_view(
                REMOTE_DEBUG_CONTROL_SERVICE_SERVICE_NAME,
                "Disconnect",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::sticky::remote::control::v1::__buffa::view::DisconnectRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::sticky::remote::control::v1::DisconnectRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.disconnect(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::sticky::remote::control::v1::DisconnectResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_DISCONNECT_SPEC)
            .route_view(
                REMOTE_DEBUG_CONTROL_SERVICE_SERVICE_NAME,
                "Shutdown",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::sticky::remote::control::v1::__buffa::view::ShutdownRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::sticky::remote::control::v1::ShutdownRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.shutdown(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::sticky::remote::control::v1::ShutdownResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_SHUTDOWN_SPEC)
    }
}
/// Type-inference marker used by [`Router::add_service`](::connectrpc::Router::add_service).
#[doc(hidden)]
pub struct RemoteDebugControlServiceRegisterMarker;
impl<
    S: RemoteDebugControlService,
> ::connectrpc::ServiceRegister<RemoteDebugControlServiceRegisterMarker>
for ::std::sync::Arc<S> {
    fn register_service(self, router: ::connectrpc::Router) -> ::connectrpc::Router {
        <S as RemoteDebugControlServiceExt>::register(self, router)
    }
}
/// Monomorphic dispatcher for `RemoteDebugControlService`.
///
/// Unlike `.register(Router)` which type-erases each method into an `Arc<dyn ErasedHandler>` stored in a `HashMap`, this struct dispatches via a compile-time `match` on method name: no vtable, no hash lookup.
///
/// # Example
///
/// ```rust,ignore
/// use connectrpc::ConnectRpcService;
///
/// let server = RemoteDebugControlServiceServer::new(MyImpl);
/// let service = ConnectRpcService::new(server);
/// // hand `service` to axum/hyper as a fallback_service
/// ```
pub struct RemoteDebugControlServiceServer<T> {
    inner: ::std::sync::Arc<T>,
}
impl<T: RemoteDebugControlService> RemoteDebugControlServiceServer<T> {
    /// Wrap a service implementation in a monomorphic dispatcher.
    pub fn new(service: T) -> Self {
        Self {
            inner: ::std::sync::Arc::new(service),
        }
    }
    /// Wrap an already-`Arc`'d service implementation.
    pub fn from_arc(inner: ::std::sync::Arc<T>) -> Self {
        Self { inner }
    }
}
impl<T> Clone for RemoteDebugControlServiceServer<T> {
    fn clone(&self) -> Self {
        Self {
            inner: ::std::sync::Arc::clone(&self.inner),
        }
    }
}
impl<T: RemoteDebugControlService> ::connectrpc::Dispatcher
for RemoteDebugControlServiceServer<T> {
    #[inline]
    fn lookup(
        &self,
        path: &str,
    ) -> Option<::connectrpc::dispatcher::codegen::MethodDescriptor> {
        let method = path
            .strip_prefix("sticky.remote.control.v1.RemoteDebugControlService/")?;
        match method {
            "Connect" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_CONNECT_SPEC),
                )
            }
            "Status" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_STATUS_SPEC),
                )
            }
            "ListTargets" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_LIST_TARGETS_SPEC),
                )
            }
            "InjectTouch" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_INJECT_TOUCH_SPEC),
                )
            }
            "InjectButton" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_INJECT_BUTTON_SPEC),
                )
            }
            "GetSnapshot" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_GET_SNAPSHOT_SPEC),
                )
            }
            "SnapshotAck" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_SNAPSHOT_ACK_SPEC),
                )
            }
            "SnapshotClear" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_SNAPSHOT_CLEAR_SPEC),
                )
            }
            "Reboot" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_REBOOT_SPEC),
                )
            }
            "Disconnect" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_DISCONNECT_SPEC),
                )
            }
            "Shutdown" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(REMOTE_DEBUG_CONTROL_SERVICE_SHUTDOWN_SPEC),
                )
            }
            _ => None,
        }
    }
    fn call_unary(
        &self,
        path: &str,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::Payload,
        format: ::connectrpc::CodecFormat,
    ) -> ::connectrpc::dispatcher::codegen::UnaryResult {
        let Some(method) = path
            .strip_prefix("sticky.remote.control.v1.RemoteDebugControlService/") else {
            return ::connectrpc::dispatcher::codegen::unimplemented_unary(path);
        };
        let _ = (&ctx, &request, &format);
        match method {
            "Connect" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::sticky::remote::control::v1::ConnectRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::sticky::remote::control::v1::__buffa::view::ConnectRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::sticky::remote::control::v1::ConnectRequest,
                    >::from_parts(&req, &body);
                    svc.connect(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::sticky::remote::control::v1::ConnectResponse,
                        >(format)
                })
            }
            "Status" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::sticky::remote::control::v1::StatusRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::sticky::remote::control::v1::__buffa::view::StatusRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::sticky::remote::control::v1::StatusRequest,
                    >::from_parts(&req, &body);
                    svc.status(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::sticky::remote::control::v1::StatusResponse,
                        >(format)
                })
            }
            "ListTargets" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::sticky::remote::control::v1::ListTargetsRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::sticky::remote::control::v1::__buffa::view::ListTargetsRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::sticky::remote::control::v1::ListTargetsRequest,
                    >::from_parts(&req, &body);
                    svc.list_targets(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::sticky::remote::control::v1::ListTargetsResponse,
                        >(format)
                })
            }
            "InjectTouch" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::sticky::remote::control::v1::InjectTouchRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::sticky::remote::control::v1::__buffa::view::InjectTouchRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::sticky::remote::control::v1::InjectTouchRequest,
                    >::from_parts(&req, &body);
                    svc.inject_touch(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::sticky::remote::control::v1::InjectTouchResponse,
                        >(format)
                })
            }
            "InjectButton" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::sticky::remote::control::v1::InjectButtonRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::sticky::remote::control::v1::__buffa::view::InjectButtonRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::sticky::remote::control::v1::InjectButtonRequest,
                    >::from_parts(&req, &body);
                    svc.inject_button(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::sticky::remote::control::v1::InjectButtonResponse,
                        >(format)
                })
            }
            "GetSnapshot" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::sticky::remote::control::v1::GetSnapshotRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::sticky::remote::control::v1::__buffa::view::GetSnapshotRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::sticky::remote::control::v1::GetSnapshotRequest,
                    >::from_parts(&req, &body);
                    svc.get_snapshot(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::sticky::remote::control::v1::GetSnapshotResponse,
                        >(format)
                })
            }
            "SnapshotAck" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::sticky::remote::control::v1::SnapshotAckRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotAckRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::sticky::remote::control::v1::SnapshotAckRequest,
                    >::from_parts(&req, &body);
                    svc.snapshot_ack(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::sticky::remote::control::v1::SnapshotAckResponse,
                        >(format)
                })
            }
            "SnapshotClear" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::sticky::remote::control::v1::SnapshotClearRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotClearRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::sticky::remote::control::v1::SnapshotClearRequest,
                    >::from_parts(&req, &body);
                    svc.snapshot_clear(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::sticky::remote::control::v1::SnapshotClearResponse,
                        >(format)
                })
            }
            "Reboot" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::sticky::remote::control::v1::RebootRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::sticky::remote::control::v1::__buffa::view::RebootRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::sticky::remote::control::v1::RebootRequest,
                    >::from_parts(&req, &body);
                    svc.reboot(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::sticky::remote::control::v1::RebootResponse,
                        >(format)
                })
            }
            "Disconnect" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::sticky::remote::control::v1::DisconnectRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::sticky::remote::control::v1::__buffa::view::DisconnectRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::sticky::remote::control::v1::DisconnectRequest,
                    >::from_parts(&req, &body);
                    svc.disconnect(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::sticky::remote::control::v1::DisconnectResponse,
                        >(format)
                })
            }
            "Shutdown" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::sticky::remote::control::v1::ShutdownRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::sticky::remote::control::v1::__buffa::view::ShutdownRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::sticky::remote::control::v1::ShutdownRequest,
                    >::from_parts(&req, &body);
                    svc.shutdown(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::sticky::remote::control::v1::ShutdownResponse,
                        >(format)
                })
            }
            _ => ::connectrpc::dispatcher::codegen::unimplemented_unary(path),
        }
    }
    fn call_server_streaming(
        &self,
        path: &str,
        ctx: ::connectrpc::RequestContext,
        request: ::buffa::bytes::Bytes,
        format: ::connectrpc::CodecFormat,
    ) -> ::connectrpc::dispatcher::codegen::StreamingResult {
        let Some(method) = path
            .strip_prefix("sticky.remote.control.v1.RemoteDebugControlService/") else {
            return ::connectrpc::dispatcher::codegen::unimplemented_streaming(path);
        };
        let _ = (&ctx, &request, &format);
        match method {
            _ => ::connectrpc::dispatcher::codegen::unimplemented_streaming(path),
        }
    }
    fn call_client_streaming(
        &self,
        path: &str,
        ctx: ::connectrpc::RequestContext,
        requests: ::connectrpc::dispatcher::codegen::RequestStream,
        format: ::connectrpc::CodecFormat,
    ) -> ::connectrpc::dispatcher::codegen::UnaryResult {
        let Some(method) = path
            .strip_prefix("sticky.remote.control.v1.RemoteDebugControlService/") else {
            return ::connectrpc::dispatcher::codegen::unimplemented_unary(path);
        };
        let _ = (&ctx, &requests, &format);
        match method {
            _ => ::connectrpc::dispatcher::codegen::unimplemented_unary(path),
        }
    }
    fn call_bidi_streaming(
        &self,
        path: &str,
        ctx: ::connectrpc::RequestContext,
        requests: ::connectrpc::dispatcher::codegen::RequestStream,
        format: ::connectrpc::CodecFormat,
    ) -> ::connectrpc::dispatcher::codegen::StreamingResult {
        let Some(method) = path
            .strip_prefix("sticky.remote.control.v1.RemoteDebugControlService/") else {
            return ::connectrpc::dispatcher::codegen::unimplemented_streaming(path);
        };
        let _ = (&ctx, &requests, &format);
        match method {
            _ => ::connectrpc::dispatcher::codegen::unimplemented_streaming(path),
        }
    }
}
/// Client for this service.
///
/// Generic over `T: ClientTransport`. For **gRPC** (HTTP/2), use
/// `Http2Connection` — it has honest `poll_ready` and composes with
/// `tower::balance` for multi-connection load balancing. For **Connect
/// over HTTP/1.1** (or unknown protocol), use `HttpClient`.
///
/// # Example (gRPC / HTTP/2)
///
/// ```rust,ignore
/// use connectrpc::client::{Http2Connection, ClientConfig};
/// use connectrpc::Protocol;
///
/// let uri: http::Uri = "http://localhost:8080".parse()?;
/// let conn = Http2Connection::connect_plaintext(uri.clone()).await?.shared(1024);
/// let config = ClientConfig::new(uri).with_protocol(Protocol::Grpc);
///
/// let client = RemoteDebugControlServiceClient::new(conn, config);
/// let response = client.connect(request).await?;
/// ```
///
/// # Example (Connect / HTTP/1.1 or ALPN)
///
/// ```rust,ignore
/// use connectrpc::client::{HttpClient, ClientConfig};
///
/// let http = HttpClient::plaintext();  // cleartext http:// only
/// let config = ClientConfig::new("http://localhost:8080".parse()?);
///
/// let client = RemoteDebugControlServiceClient::new(http, config);
/// let response = client.connect(request).await?;
/// ```
///
/// # Working with the response
///
/// Unary calls return [`UnaryResponse<OwnedView<FooView>>`](::connectrpc::client::UnaryResponse).
/// [`view()`](::connectrpc::client::UnaryResponse::view) borrows the response
/// message, so field access is zero-copy:
///
/// ```rust,ignore
/// let resp = client.connect(request).await?;
/// let name: &str = resp.view().name;  // borrow into the response buffer
/// ```
///
/// If you need the owned struct (e.g. to store or pass by value), use
/// [`into_owned()`](::connectrpc::client::UnaryResponse::into_owned):
///
/// ```rust,ignore
/// let owned = client.connect(request).await?.into_owned();
/// ```
///
/// [`into_view()`](::connectrpc::client::UnaryResponse::into_view) keeps the
/// zero-copy decoded body (an `OwnedView`) without copying; field access on it
/// goes through `.reborrow()`. Streaming responses yield one
/// [`StreamMessage`](::connectrpc::StreamMessage) per received message from
/// `.message().await` — read fields zero-copy through the generated accessor
/// methods (`msg.name()`) or `.view()`, or convert with `.to_owned_message()`.
#[derive(Clone)]
pub struct RemoteDebugControlServiceClient<T> {
    transport: T,
    config: ::connectrpc::client::ClientConfig,
}
impl<T> RemoteDebugControlServiceClient<T>
where
    T: ::connectrpc::client::ClientTransport,
    <T::ResponseBody as ::connectrpc::http_body::Body>::Error: ::std::fmt::Display,
{
    /// Create a new client with the given transport and configuration.
    pub fn new(transport: T, config: ::connectrpc::client::ClientConfig) -> Self {
        Self { transport, config }
    }
    /// Get the client configuration.
    pub fn config(&self) -> &::connectrpc::client::ClientConfig {
        &self.config
    }
    /// Get a mutable reference to the client configuration.
    pub fn config_mut(&mut self) -> &mut ::connectrpc::client::ClientConfig {
        &mut self.config
    }
    /// Call the Connect RPC. Sends a request to /sticky.remote.control.v1.RemoteDebugControlService/Connect.
    pub async fn connect(
        &self,
        request: crate::proto::sticky::remote::control::v1::ConnectRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::ConnectResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.connect_with_options(request, ::connectrpc::client::CallOptions::default())
            .await
    }
    /// Call the Connect RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn connect_with_options(
        &self,
        request: crate::proto::sticky::remote::control::v1::ConnectRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::ConnectResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                REMOTE_DEBUG_CONTROL_SERVICE_CONNECT_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
    /// Call the Status RPC. Sends a request to /sticky.remote.control.v1.RemoteDebugControlService/Status.
    pub async fn status(
        &self,
        request: crate::proto::sticky::remote::control::v1::StatusRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::StatusResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.status_with_options(request, ::connectrpc::client::CallOptions::default())
            .await
    }
    /// Call the Status RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn status_with_options(
        &self,
        request: crate::proto::sticky::remote::control::v1::StatusRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::StatusResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                REMOTE_DEBUG_CONTROL_SERVICE_STATUS_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
    /// Call the ListTargets RPC. Sends a request to /sticky.remote.control.v1.RemoteDebugControlService/ListTargets.
    pub async fn list_targets(
        &self,
        request: crate::proto::sticky::remote::control::v1::ListTargetsRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::ListTargetsResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.list_targets_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the ListTargets RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn list_targets_with_options(
        &self,
        request: crate::proto::sticky::remote::control::v1::ListTargetsRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::ListTargetsResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                REMOTE_DEBUG_CONTROL_SERVICE_LIST_TARGETS_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
    /// Call the InjectTouch RPC. Sends a request to /sticky.remote.control.v1.RemoteDebugControlService/InjectTouch.
    pub async fn inject_touch(
        &self,
        request: crate::proto::sticky::remote::control::v1::InjectTouchRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::InjectTouchResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.inject_touch_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the InjectTouch RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn inject_touch_with_options(
        &self,
        request: crate::proto::sticky::remote::control::v1::InjectTouchRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::InjectTouchResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                REMOTE_DEBUG_CONTROL_SERVICE_INJECT_TOUCH_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
    /// Call the InjectButton RPC. Sends a request to /sticky.remote.control.v1.RemoteDebugControlService/InjectButton.
    pub async fn inject_button(
        &self,
        request: crate::proto::sticky::remote::control::v1::InjectButtonRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::InjectButtonResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.inject_button_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the InjectButton RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn inject_button_with_options(
        &self,
        request: crate::proto::sticky::remote::control::v1::InjectButtonRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::InjectButtonResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                REMOTE_DEBUG_CONTROL_SERVICE_INJECT_BUTTON_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
    /// Call the GetSnapshot RPC. Sends a request to /sticky.remote.control.v1.RemoteDebugControlService/GetSnapshot.
    pub async fn get_snapshot(
        &self,
        request: crate::proto::sticky::remote::control::v1::GetSnapshotRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::GetSnapshotResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.get_snapshot_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the GetSnapshot RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn get_snapshot_with_options(
        &self,
        request: crate::proto::sticky::remote::control::v1::GetSnapshotRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::GetSnapshotResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                REMOTE_DEBUG_CONTROL_SERVICE_GET_SNAPSHOT_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
    /// Call the SnapshotAck RPC. Sends a request to /sticky.remote.control.v1.RemoteDebugControlService/SnapshotAck.
    pub async fn snapshot_ack(
        &self,
        request: crate::proto::sticky::remote::control::v1::SnapshotAckRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotAckResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.snapshot_ack_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the SnapshotAck RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn snapshot_ack_with_options(
        &self,
        request: crate::proto::sticky::remote::control::v1::SnapshotAckRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotAckResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                REMOTE_DEBUG_CONTROL_SERVICE_SNAPSHOT_ACK_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
    /// Call the SnapshotClear RPC. Sends a request to /sticky.remote.control.v1.RemoteDebugControlService/SnapshotClear.
    pub async fn snapshot_clear(
        &self,
        request: crate::proto::sticky::remote::control::v1::SnapshotClearRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotClearResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.snapshot_clear_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the SnapshotClear RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn snapshot_clear_with_options(
        &self,
        request: crate::proto::sticky::remote::control::v1::SnapshotClearRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::SnapshotClearResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                REMOTE_DEBUG_CONTROL_SERVICE_SNAPSHOT_CLEAR_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
    /// Call the Reboot RPC. Sends a request to /sticky.remote.control.v1.RemoteDebugControlService/Reboot.
    pub async fn reboot(
        &self,
        request: crate::proto::sticky::remote::control::v1::RebootRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::RebootResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.reboot_with_options(request, ::connectrpc::client::CallOptions::default())
            .await
    }
    /// Call the Reboot RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn reboot_with_options(
        &self,
        request: crate::proto::sticky::remote::control::v1::RebootRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::RebootResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                REMOTE_DEBUG_CONTROL_SERVICE_REBOOT_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
    /// Call the Disconnect RPC. Sends a request to /sticky.remote.control.v1.RemoteDebugControlService/Disconnect.
    pub async fn disconnect(
        &self,
        request: crate::proto::sticky::remote::control::v1::DisconnectRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::DisconnectResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.disconnect_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the Disconnect RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn disconnect_with_options(
        &self,
        request: crate::proto::sticky::remote::control::v1::DisconnectRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::DisconnectResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                REMOTE_DEBUG_CONTROL_SERVICE_DISCONNECT_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
    /// Call the Shutdown RPC. Sends a request to /sticky.remote.control.v1.RemoteDebugControlService/Shutdown.
    pub async fn shutdown(
        &self,
        request: crate::proto::sticky::remote::control::v1::ShutdownRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::ShutdownResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.shutdown_with_options(request, ::connectrpc::client::CallOptions::default())
            .await
    }
    /// Call the Shutdown RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn shutdown_with_options(
        &self,
        request: crate::proto::sticky::remote::control::v1::ShutdownRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::sticky::remote::control::v1::__buffa::view::ShutdownResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                REMOTE_DEBUG_CONTROL_SERVICE_SHUTDOWN_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
}
