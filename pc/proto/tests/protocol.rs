//! `pc/proto` 的协议单测。
//!
//! 覆盖四类必须成立的性质（`docs/plans/2026-10-07-agent-slicing.md` 的 S2 验收）：
//!
//! 1. 帧头**小端** + 长度字段**只算负载**；
//! 2. **CRC 覆盖类型** —— 换个类型解码必须失败；
//! 3. 负载被改动**必被发现**；
//! 4. DTO 的 JSON 往返（含字段名与协议逐字一致）。
//!
//! 另外用**独立实现的标准 CRC-32** 交叉验证 `crc32fast` 的结果，
//! 并用**手工拼出的 agent 侧字节**验证两侧逐字节对齐。

use std::io::Cursor;

use proto::wire::{kind, kind_name};
use proto::{
    decode_control, encode_control, parse_frame_header, parse_video_payload, read_frame,
    write_frame, AppliedScreen, ApplyScreenArgs, ApplyScreenResult, BitrateMode, Caps, CapsDefaults,
    CapsRequest, Codec, CodecCaps, ColorRange, ColorStandard, ColorTransfer, Command, CommandAck,
    CommandBody, ErrorObj, Event, FrameHeader, Heartbeat, Hello, HelloAck, InputAction, InputArgs,
    InputKey, InputPointer, InputScroll, IntRange, ProtocolError, ScreenParams, FLAG_KEYFRAME,
    HEADER_SIZE, MAX_PAYLOAD, PROTOCOL_VERSION, VIDEO_HEADER_SIZE,
};

// ---------------------------------------------------------------- 参考实现

/// 独立实现的 CRC-32（IEEE 802.3）—— 逐位算法，与任何库都无关。
///
/// 存在理由：被测代码用的 `crc32fast` 是查表实现，用它自己的结果当期望值就是循环论证。
fn reference_crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// 按协议拼出 CRC 的输入：`[类型][4B 小端负载长度][JSON]`。
fn crc_input(kind: u8, payload_len: usize, body: &[u8]) -> Vec<u8> {
    let mut v = vec![kind];
    v.extend_from_slice(&(payload_len as u32).to_le_bytes());
    v.extend_from_slice(body);
    v
}

/// 只填 `streamId` 的 `applied`。
fn minimal_applied(stream_id: i32) -> AppliedScreen {
    AppliedScreen {
        stream_id,
        width: None,
        height: None,
        refresh_rate: None,
        dpi: None,
        bitrate: None,
        bitrate_mode: None,
        codec: None,
        codec_name: None,
        bit_depth: None,
        profile: None,
        color_standard: None,
        color_range: None,
        color_transfer: None,
        show_system_decorations: None,
        always_unlocked: None,
        own_display_group: None,
        screen_name: None,
    }
}

// ---------------------------------------------------------------- CRC 算法

#[test]
fn reference_crc_matches_standard_vector() {
    // 经典向量：CRC-32("123456789") = 0xCBF43926（`java.util.zip.CRC32` 同值）。
    assert_eq!(reference_crc32(b"123456789"), 0xCBF4_3926);
}

#[test]
fn crc32_matches_reference_bit_by_bit() {
    let body = b"{\"version\":1}";
    for kind in [kind::HELLO, kind::CAPS, kind::HEARTBEAT, 0x7F] {
        for extra in 0..4usize {
            let payload_len = 4 + body.len() + extra;
            assert_eq!(
                proto::crc32(kind, payload_len, body),
                reference_crc32(&crc_input(kind, payload_len, body)),
                "kind={kind:#04x} payload_len={payload_len}"
            );
        }
    }
}

// ---------------------------------------------------------------- 1. 帧头

#[test]
fn frame_header_is_little_endian_and_length_excludes_header() {
    let payload = [0xAAu8, 0xBB, 0xCC];
    let mut out = Vec::new();
    write_frame(&mut out, kind::FRAME, &payload).unwrap();

    // 头 5 字节：类型 + 负载长度（小端），**不含头自身**。
    assert_eq!(out[0], kind::FRAME);
    assert_eq!(&out[1..5], &[3, 0, 0, 0]);
    assert_eq!(out.len(), HEADER_SIZE + payload.len());
    assert_eq!(&out[5..], &payload);

    // 往返
    let mut cur = Cursor::new(&out);
    let frame = read_frame(&mut cur).unwrap();
    assert_eq!(frame.kind, kind::FRAME);
    assert_eq!(frame.payload, payload);
    assert_eq!(cur.position() as usize, out.len());
}

#[test]
fn length_field_is_parsed_as_little_endian() {
    // 0x0001_0000 = 65536 —— 只有正确按小端解析才对得上。
    let mut header = [0u8; HEADER_SIZE];
    header[0] = kind::FRAME;
    header[1..5].copy_from_slice(&65_536u32.to_le_bytes());
    let parsed = parse_frame_header(&header).unwrap();
    assert_eq!(parsed.length, 65_536);
    assert_eq!(parsed.kind, kind::FRAME);
}

#[test]
fn zero_length_payload_is_valid() {
    let mut out = Vec::new();
    write_frame(&mut out, kind::HEARTBEAT, &[]).unwrap();
    assert_eq!(out.len(), HEADER_SIZE);
    assert_eq!(&out[1..5], &[0, 0, 0, 0]);

    let mut cur = Cursor::new(&out);
    let frame = read_frame(&mut cur).unwrap();
    assert!(frame.payload.is_empty());
    assert_eq!(frame.kind, kind::HEARTBEAT);
}

#[test]
fn oversized_length_is_rejected_before_allocating() {
    // 头部声明 16 MiB + 1：必须在读负载之前就报错（否则就是对端炸内存的入口）。
    let mut header = [0u8; HEADER_SIZE];
    header[0] = kind::FRAME;
    header[1..5].copy_from_slice(&((MAX_PAYLOAD as u32) + 1).to_le_bytes());

    let mut cur = Cursor::new(&header[..]);
    match read_frame(&mut cur) {
        Err(ProtocolError::PayloadTooLarge { length, limit }) => {
            assert_eq!(length, MAX_PAYLOAD as u64 + 1);
            assert_eq!(limit, MAX_PAYLOAD);
        }
        other => panic!("期望 PayloadTooLarge，得到 {other:?}"),
    }
    // 只消费了 5 字节头 ⇒ 负载一个字节都没读、更没分配。
    assert_eq!(cur.position(), HEADER_SIZE as u64);

    // 负长度（Java 侧的有符号 Int 视角）换算成 u32 后必然超限。
    let mut negative = [0u8; HEADER_SIZE];
    negative[0] = kind::FRAME;
    negative[1..5].copy_from_slice(&(-1i32).to_le_bytes());
    assert!(matches!(
        parse_frame_header(&negative),
        Err(ProtocolError::PayloadTooLarge { .. })
    ));

    // 恰好等于上限是允许的（边界不能误杀）。
    let mut boundary = [0u8; HEADER_SIZE];
    boundary[0] = kind::FRAME;
    boundary[1..5].copy_from_slice(&(MAX_PAYLOAD as u32).to_le_bytes());
    assert_eq!(parse_frame_header(&boundary).unwrap().length as usize, MAX_PAYLOAD);

    // 写出超限的帧同样被拒。
    assert!(matches!(
        FrameHeader { kind: kind::FRAME, length: (MAX_PAYLOAD as u32) + 1 }.to_bytes(),
        Err(ProtocolError::PayloadTooLarge { .. })
    ));
}

#[test]
fn truncated_stream_reports_how_much_was_read() {
    // 头声明 8 字节负载，但只给了 3 字节。
    let bytes = vec![kind::FRAME, 8, 0, 0, 0, 1, 2, 3];
    let mut cur = Cursor::new(bytes);
    match read_frame(&mut cur) {
        Err(ProtocolError::PeerClosed { got, expected }) => {
            assert_eq!(got, 3);
            assert_eq!(expected, 8);
        }
        other => panic!("期望 PeerClosed，得到 {other:?}"),
    }
}

// ---------------------------------------------------------------- 2/3. CRC

#[test]
fn crc_covers_the_type_so_decoding_with_another_type_fails() {
    let payload = encode_control(kind::HELLO, &Hello { version: PROTOCOL_VERSION }).unwrap();

    // 同类型解码成功。
    let ok: Hello = decode_control(kind::HELLO, &payload).unwrap();
    assert_eq!(ok.version, PROTOCOL_VERSION);

    // 换成任何别的类型解码都必须失败 —— 类型参与了 CRC。
    for other in [kind::HELLO_ACK, kind::CAPS_REQUEST, kind::COMMAND, kind::HEARTBEAT] {
        match decode_control::<Hello>(other, &payload) {
            Err(ProtocolError::CrcMismatch { .. }) => {}
            res => panic!("类型 {other:#04x} 不该通过校验：{res:?}"),
        }
    }
}

#[test]
fn crc_input_contains_the_whole_payload_length() {
    let msg = Hello { version: PROTOCOL_VERSION };
    let payload = encode_control(kind::HELLO, &msg).unwrap();
    let json = serde_json::to_vec(&msg).unwrap();

    assert_eq!(payload.len(), 4 + json.len());
    // 长度字段喂进 CRC 的是**整个负载长度**（含 CRC 那 4 字节）。
    let expected = reference_crc32(&crc_input(kind::HELLO, payload.len(), &json));
    assert_eq!(u32::from_le_bytes(payload[..4].try_into().unwrap()), expected);
    assert_eq!(&payload[4..], &json[..]);
}

#[test]
fn any_tampering_of_the_payload_is_detected() {
    let payload = encode_control(kind::CAPS_REQUEST, &CapsRequest { request_id: 3 }).unwrap();

    // ① 改 JSON 里的一个字节。
    for idx in 4..payload.len() {
        let mut tampered = payload.clone();
        tampered[idx] ^= 0x01;
        assert!(
            matches!(
                decode_control::<CapsRequest>(kind::CAPS_REQUEST, &tampered),
                Err(ProtocolError::CrcMismatch { .. })
            ),
            "第 {idx} 字节被改动却没被发现"
        );
    }

    // ② 改 CRC 本身。
    let mut tampered = payload.clone();
    tampered[0] ^= 0xFF;
    assert!(matches!(
        decode_control::<CapsRequest>(kind::CAPS_REQUEST, &tampered),
        Err(ProtocolError::CrcMismatch { .. })
    ));

    // ③ 截断（长度变了，CRC 的输入也变了）。
    let truncated = &payload[..payload.len() - 1];
    assert!(matches!(
        decode_control::<CapsRequest>(kind::CAPS_REQUEST, truncated),
        Err(ProtocolError::CrcMismatch { .. })
    ));

    // ④ 短于 CRC 字段。
    assert!(matches!(
        decode_control::<CapsRequest>(kind::CAPS_REQUEST, &[0, 1, 2]),
        Err(ProtocolError::PayloadTooShort { need: 4, got: 3 })
    ));
}

#[test]
fn byte_for_byte_alignment_with_the_agent_side() {
    // 模拟 Kotlin 侧 `Codec.encode(TYPE_HELLO, HelloMsg(1))` 的输出：
    // json = {"version":1}，负载长度 = 4 + 13 = 17，CRC 覆盖 [0x10][11 00 00 00][json]。
    let json = br#"{"version":1}"#;
    let payload_len = 4 + json.len();
    assert_eq!(payload_len, 17);
    let crc = reference_crc32(&crc_input(kind::HELLO, payload_len, json));

    let mut wire = vec![kind::HELLO];
    wire.extend_from_slice(&(payload_len as u32).to_le_bytes());
    wire.extend_from_slice(&crc.to_le_bytes());
    wire.extend_from_slice(json);

    let mut cur = Cursor::new(&wire[..]);
    let frame = read_frame(&mut cur).unwrap();
    assert_eq!(frame.kind, kind::HELLO);
    assert_eq!(frame.payload.len(), 17);
    let hello: Hello = decode_control(frame.kind, &frame.payload).unwrap();
    assert_eq!(hello.version, 1);

    // 反向：本端编出的负载必须与手工拼的完全一致。
    assert_eq!(encode_control(kind::HELLO, &Hello { version: 1 }).unwrap(), frame.payload);
}

// ---------------------------------------------------------------- 4. DTO

#[test]
fn handshake_dtos_round_trip() {
    let hello = Hello { version: PROTOCOL_VERSION };
    assert_eq!(serde_json::to_string(&hello).unwrap(), r#"{"version":1}"#);
    let back: Hello =
        decode_control(kind::HELLO, &encode_control(kind::HELLO, &hello).unwrap()).unwrap();
    assert_eq!(back, hello);

    let ack = HelloAck { version: PROTOCOL_VERSION, state: HelloAck::STATE_READY.into() };
    assert_eq!(serde_json::to_string(&ack).unwrap(), r#"{"version":1,"state":"ready"}"#);
    let back: HelloAck =
        decode_control(kind::HELLO_ACK, &encode_control(kind::HELLO_ACK, &ack).unwrap()).unwrap();
    assert_eq!(back, ack);

    let req = CapsRequest { request_id: 3 };
    assert_eq!(serde_json::to_string(&req).unwrap(), r#"{"requestId":3}"#);
    let back: CapsRequest =
        decode_control(kind::CAPS_REQUEST, &encode_control(kind::CAPS_REQUEST, &req).unwrap())
            .unwrap();
    assert_eq!(back, req);

    // 心跳负载是空对象。
    assert_eq!(serde_json::to_string(&Heartbeat {}).unwrap(), "{}");
    let back: Heartbeat =
        decode_control(kind::HEARTBEAT, &encode_control(kind::HEARTBEAT, &Heartbeat {}).unwrap())
            .unwrap();
    assert_eq!(back, Heartbeat {});
}

#[test]
fn apply_screen_command_round_trips() {
    let cmd = Command::apply_screen(
        7,
        0,
        ScreenParams {
            width: Some(1920),
            height: Some(1080),
            refresh_rate: Some(60),
            dpi: Some(320),
            bitrate_bpp: Some(0.1),
            bitrate_mode: Some(BitrateMode::Vbr),
            codec: Some(Codec::H264),
            bit_depth: Some(8),
            profile: Some("High".into()),
            color_standard: Some(ColorStandard::Bt709),
            color_range: Some(ColorRange::Full),
            color_transfer: Some(ColorTransfer::SdrVideo),
            show_system_decorations: Some(false),
            always_unlocked: Some(true),
            screen_name: Some("WCReen".into()),
            ..Default::default()
        },
    );

    // 线形状**逐字**钉死：信封（`requestId` + `cmd`）+ `payload` 里的载荷结构体。
    // 载荷里的字段名与协议逐字一致（`streamId` / `params` + camelCase）。
    let json = serde_json::to_string(&cmd).unwrap();
    assert_eq!(
        json,
        concat!(
            r#"{"requestId":7,"cmd":"applyScreen","payload":{"streamId":0,"params":{"#,
            r#""width":1920,"height":1080,"refreshRate":60,"dpi":320,"bitrateBpp":0.1,"#,
            r#""bitrateMode":"VBR","codec":"H.264","bitDepth":8,"profile":"High","#,
            r#""colorStandard":"BT709","colorRange":"FULL","colorTransfer":"SDR_VIDEO","#,
            r#""showSystemDecorations":false,"alwaysUnlocked":true,"screenName":"WCReen"}}}"#,
        )
    );
    // 未指定的字段（这里是 `bitrate`）不得出现 —— 不发看起来像真值的 0。
    assert!(!json.contains(r#""bitrate":"#), "{json}");

    // 两段式解析：线字节 → 信封 + 载荷结构体，逐字段相等。
    let payload = encode_control(kind::COMMAND, &cmd).unwrap();
    let back: Command = decode_control(kind::COMMAND, &payload).unwrap();
    assert_eq!(back, cmd);
    assert_eq!(back.request_id(), 7);
    match &back.body {
        CommandBody::ApplyScreen(ApplyScreenArgs { stream_id, params }) => {
            assert_eq!(*stream_id, 0);
            assert_eq!(params.width, Some(1920));
        }
        other => panic!("不是 applyScreen：{other:?}"),
    }
}

#[test]
fn apply_screen_params_may_all_be_absent() {
    let cmd = Command::apply_screen(1, 0, ScreenParams::default());
    assert_eq!(
        serde_json::to_string(&cmd).unwrap(),
        r#"{"requestId":1,"cmd":"applyScreen","payload":{"streamId":0,"params":{}}}"#
    );
    let back: Command =
        decode_control(kind::COMMAND, &encode_control(kind::COMMAND, &cmd).unwrap()).unwrap();
    assert_eq!(back, cmd);
}

#[test]
fn caps_table_round_trips() {
    let caps = Caps {
        request_id: 3,
        codecs: vec![CodecCaps {
            name: "c2.qti.avc.encoder".into(),
            mime: "video/avc".into(),
            codec: Codec::H264,
            hardware: true,
            bit_depths: vec![8],
            profiles: vec!["Baseline".into(), "Main".into(), "High".into()],
            color_standards: vec![ColorStandard::Bt709, ColorStandard::Bt601Ntsc],
            color_ranges: vec![ColorRange::Limited, ColorRange::Full],
            color_transfers: vec![ColorTransfer::SdrVideo],
            width: IntRange { min: 128, max: 8192, step: 2 },
            height: IntRange { min: 128, max: 8192, step: 2 },
            refresh_rate: IntRange { min: 1, max: 480, step: 1 },
            bitrate: IntRange { min: 1, max: 220_000_000, step: 1 },
            max_instances: 16,
        }],
        defaults: CapsDefaults {
            codec: Codec::H264,
            bit_depth: 8,
            profile: "High".into(),
            color_standard: ColorStandard::Bt709,
            color_range: ColorRange::Full,
            color_transfer: ColorTransfer::SdrVideo,
            bitrate_bpp: 0.1,
            bitrate_mode: BitrateMode::Vbr,
            refresh_rate: 60,
            i_frame_interval: 1,
            show_system_decorations: false,
            always_unlocked: true,
        },
    };

    let json = serde_json::to_string(&caps).unwrap();
    for needle in [
        r#""requestId":3"#,
        r#""codecs":[{"#,
        r#""name":"c2.qti.avc.encoder""#,
        r#""mime":"video/avc""#,
        r#""hardware":true"#,
        r#""bitDepths":[8]"#,
        r#""colorStandards":["BT709","BT601_NTSC"]"#,
        r#""width":{"min":128,"max":8192,"step":2}"#,
        r#""maxInstances":16"#,
        r#""defaults":{"#,
        r#""iFrameInterval":1"#,
    ] {
        assert!(json.contains(needle), "缺字段 {needle}：{json}");
    }
    // 契约里没有顶层 `ranges` / `formatGroups`。
    assert!(!json.contains(r#""ranges""#) && !json.contains(r#""formatGroups""#), "{json}");

    let payload = encode_control(kind::CAPS, &caps).unwrap();
    let back: Caps = decode_control(kind::CAPS, &payload).unwrap();
    assert_eq!(back, caps);
}

#[test]
fn command_ack_round_trips() {
    // 成功：result 里带 streamId + applied。
    let ok = CommandAck {
        request_id: 7,
        ok: true,
        result: Some(ApplyScreenResult {
            stream_id: 1,
            applied: AppliedScreen {
                stream_id: 1,
                width: Some(1920),
                height: Some(1080),
                refresh_rate: Some(60.0),
                dpi: Some(320),
                bitrate: Some(12_345_678),
                bitrate_mode: Some(BitrateMode::Vbr),
                codec: Some(Codec::H264),
                codec_name: Some("c2.qti.avc.encoder".into()),
                bit_depth: Some(8),
                profile: Some("High".into()),
                color_standard: Some(ColorStandard::Bt709),
                color_range: Some(ColorRange::Full),
                color_transfer: Some(ColorTransfer::SdrVideo),
                show_system_decorations: Some(false),
                always_unlocked: Some(true),
                own_display_group: Some(true),
                screen_name: Some("WCReen-1".into()),
            },
        }),
        applied: None,
        error: None,
    };
    let json = serde_json::to_string(&ok).unwrap();
    assert!(json.contains(r#""result":{"streamId":1,"applied":{"#), "{json}");
    let back: CommandAck =
        decode_control(kind::COMMAND_ACK, &encode_control(kind::COMMAND_ACK, &ok).unwrap())
            .unwrap();
    assert_eq!(back, ok);

    // 失败：允许部分成功 ⇒ `ok:false` + `error` + `applied`。
    let bad = CommandAck {
        request_id: 8,
        ok: false,
        result: None,
        applied: Some(AppliedScreen {
            bitrate: Some(8_000_000),
            ..minimal_applied(2)
        }),
        error: Some(ErrorObj {
            class: ErrorObj::CLASS_RECOVERABLE.into(),
            code: "sizeUnsupported".into(),
            message: "1920x1081 不满足 2 像素对齐".into(),
        }),
    };
    let json = serde_json::to_string(&bad).unwrap();
    assert!(json.contains(r#""ok":false"#), "{json}");
    assert!(json.contains(r#""class":"recoverable""#), "{json}");
    assert!(json.contains(r#""applied":{"#), "{json}");
    let back: CommandAck =
        decode_control(kind::COMMAND_ACK, &encode_control(kind::COMMAND_ACK, &bad).unwrap())
            .unwrap();
    assert_eq!(back, bad);

    // 回报里 `applied` 只有 streamId 是必需的（其余字段缺省不影响解析）。
    let lean: CommandAck = serde_json::from_str(
        r#"{"requestId":9,"ok":true,"result":{"streamId":3,"applied":{"streamId":3}}}"#,
    )
    .unwrap();
    let result = lean.result.unwrap();
    assert_eq!(result.stream_id, 3);
    assert_eq!(result.applied.stream_id, 3);
    assert_eq!(result.applied.width, None);
}

#[test]
fn event_and_error_objects_round_trip() {
    let released = Event {
        event: Event::SCREEN_RELEASED.into(),
        stream_id: Some(2),
        reason: Some(Event::REASON_N_ACCESS_FAILURES.into()),
        subsystem: None,
    };
    assert_eq!(
        serde_json::to_string(&released).unwrap(),
        r#"{"event":"screenReleased","streamId":2,"reason":"nAccessFailures"}"#
    );
    let back: Event =
        decode_control(kind::EVENT, &encode_control(kind::EVENT, &released).unwrap()).unwrap();
    assert_eq!(back, released);

    let fatal = Event {
        event: Event::FATAL.into(),
        stream_id: None,
        reason: Some("encoder init failed".into()),
        subsystem: None,
    };
    let back: Event =
        decode_control(kind::EVENT, &encode_control(kind::EVENT, &fatal).unwrap()).unwrap();
    assert_eq!(back, fatal);

    let unavailable = Event {
        event: Event::SUBSYSTEM_UNAVAILABLE.into(),
        stream_id: None,
        reason: None,
        subsystem: Some("encoder".into()),
    };
    let back: Event =
        decode_control(kind::EVENT, &encode_control(kind::EVENT, &unavailable).unwrap()).unwrap();
    assert_eq!(back, unavailable);

    // 错误对象嵌在指令回报里。
    let err = ErrorObj {
        class: ErrorObj::CLASS_STREAM.into(),
        code: "versionMismatch".into(),
        message: "协议版本不匹配：本机 1，对端 0 —— 请重推 agent".into(),
    };
    let ack = CommandAck {
        request_id: 1,
        ok: false,
        result: None,
        applied: None,
        error: Some(err.clone()),
    };
    let back: CommandAck =
        decode_control(kind::COMMAND_ACK, &encode_control(kind::COMMAND_ACK, &ack).unwrap())
            .unwrap();
    assert_eq!(back, ack);
}

#[test]
fn unknown_fields_are_ignored_but_unknown_enum_values_fail() {
    // 对端加了字段 ⇒ 本端仍能解析（前向兼容）。
    let ack: HelloAck =
        serde_json::from_str(r#"{"version":1,"state":"ready","future":42}"#).unwrap();
    assert_eq!(ack.state, HelloAck::STATE_READY);

    // 未知枚举取值 ⇒ 明确失败（协议版本精确匹配，取值集合不同说明对端不是这个版本）。
    let defaults = r#"{"codec":"AV1","bitDepth":8,"profile":"High","colorStandard":"BT709","colorRange":"FULL","colorTransfer":"SDR_VIDEO","bitrateBpp":0.1,"bitrateMode":"VBR","refreshRate":60,"iFrameInterval":1,"showSystemDecorations":false,"alwaysUnlocked":true}"#;
    assert!(serde_json::from_str::<CapsDefaults>(defaults).is_err());
    assert!(
        serde_json::from_str::<CapsDefaults>(&defaults.replace("\"AV1\"", "\"H.264\"")).is_ok()
    );
}

// ---------------------------------------------------------------- 视频帧负载

#[test]
fn video_frame_payload_round_trips() {
    let annex_b = [0x00u8, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x1E];
    let payload = proto::encode_video_payload(true, 1_234_567, &annex_b);
    assert_eq!(payload.len(), VIDEO_HEADER_SIZE + annex_b.len());
    assert_eq!(payload[0], FLAG_KEYFRAME);
    assert_eq!(&payload[1..9], &1_234_567i64.to_le_bytes());
    assert_eq!(&payload[9..], &annex_b);

    let (header, data) = parse_video_payload(&payload).unwrap();
    assert!(header.keyframe);
    assert_eq!(header.pts_us, 1_234_567);
    assert_eq!(data, annex_b);

    // 非关键帧：flags 只有 bit0 有定义。
    let payload = proto::encode_video_payload(false, -1, &annex_b);
    assert_eq!(payload[0], 0);
    let (header, _) = parse_video_payload(&payload).unwrap();
    assert!(!header.keyframe);
    assert_eq!(header.pts_us, -1);

    // 保留位不得被当成关键帧。
    let mut reserved = payload.clone();
    reserved[0] = 0xFE;
    assert!(!parse_video_payload(&reserved).unwrap().0.keyframe);

    // 太短必须报错，而不是读到越界。
    assert!(matches!(
        parse_video_payload(&[0u8; VIDEO_HEADER_SIZE - 1]),
        Err(ProtocolError::PayloadTooShort { need: 9, got: 8 })
    ));
}

#[test]
fn input_command_has_the_exact_wire_shape() {
    // 触摸：`action` / `pointers` 都在 `payload` 里 —— 其余载荷字段**必须不出现**
    // （agent 按 action 决定读哪个载荷）。
    let down = Command::input(
        7,
        InputArgs {
            action: InputAction::Down,
            pointers: Some(vec![InputPointer { id: 0, x: 640, y: 360 }]),
            scroll: None,
            key: None,
        },
    );
    let json = serde_json::to_string(&down).unwrap();
    assert_eq!(
        json,
        r#"{"requestId":7,"cmd":"input","payload":{"action":"down","pointers":[{"id":0,"x":640,"y":360}]}}"#
    );
    // 对端发来的同一形状也必须能读回（两段式解析：信封 → `payload` 载荷结构体）
    let back: Command = serde_json::from_str(&json).unwrap();
    assert_eq!(back, down);
    assert_eq!(back.request_id(), 7);
    match &back.body {
        CommandBody::Input(InputArgs { action, pointers, .. }) => {
            assert_eq!(*action, InputAction::Down);
            assert_eq!(pointers.as_ref().map(Vec::len), Some(1));
        }
        other => panic!("不是 input：{other:?}"),
    }

    // 滚轮：增量 + 位置（Android 的 `ACTION_SCROLL` 要用坐标做命中判定）
    let scroll = Command::input(
        8,
        InputArgs {
            action: InputAction::Scroll,
            pointers: None,
            scroll: Some(InputScroll { dx: 0, dy: -120, x: 10, y: 20 }),
            key: None,
        },
    );
    assert_eq!(
        serde_json::to_string(&scroll).unwrap(),
        r#"{"requestId":8,"cmd":"input","payload":{"action":"scroll","scroll":{"dx":0,"dy":-120,"x":10,"y":20}}}"#
    );

    // 按键：`keyDown` 的拼写是**协议的一部分**（agent 侧逐字匹配）
    let key = Command::input(
        9,
        InputArgs {
            action: InputAction::KeyDown,
            pointers: None,
            scroll: None,
            key: Some(InputKey { code: 4, meta: 0 }),
        },
    );
    assert_eq!(
        serde_json::to_string(&key).unwrap(),
        r#"{"requestId":9,"cmd":"input","payload":{"action":"keyDown","key":{"code":4,"meta":0}}}"#
    );

    // 抬起：`pointers` 是**空数组**（"抬起后没有剩下的触点"），不能省成 null ——
    // agent 侧的推导要区分「给了一个空集合」与「压根没给」。
    let up = Command::input(
        10,
        InputArgs {
            action: InputAction::Up,
            pointers: Some(Vec::new()),
            scroll: None,
            key: None,
        },
    );
    assert_eq!(
        serde_json::to_string(&up).unwrap(),
        r#"{"requestId":10,"cmd":"input","payload":{"action":"up","pointers":[]}}"#
    );
}

#[test]
fn fieldless_commands_omit_the_payload() {
    // 无字段指令：**不带 `payload`**（unit 变体，而不是编成 `"payload":{}`）。
    let stop = Command::stop_agent(9);
    assert_eq!(serde_json::to_string(&stop).unwrap(), r#"{"requestId":9,"cmd":"stopAgent"}"#);
    let back: Command = serde_json::from_str(r#"{"requestId":9,"cmd":"stopAgent"}"#).unwrap();
    assert_eq!(back, stop);
    assert_eq!(back.request_id(), 9);
    assert!(matches!(back.body, CommandBody::StopAgent));

    let sync = Command::request_sync_frame(11);
    assert_eq!(serde_json::to_string(&sync).unwrap(), r#"{"requestId":11,"cmd":"requestSyncFrame"}"#);
    let back: Command =
        decode_control(kind::COMMAND, &encode_control(kind::COMMAND, &sync).unwrap()).unwrap();
    assert_eq!(back, sync);
    assert!(matches!(back.body, CommandBody::RequestSyncFrame));
}

#[test]
fn every_input_action_has_its_protocol_spelling() {
    // 七个取值逐字对应 `docs/spec/agent/transport-shapes.md` §5 —— 改它们就是改协议。
    for (action, want) in [
        (InputAction::Down, "down"),
        (InputAction::Move, "move"),
        (InputAction::Up, "up"),
        (InputAction::Cancel, "cancel"),
        (InputAction::Scroll, "scroll"),
        (InputAction::KeyDown, "keyDown"),
        (InputAction::KeyUp, "keyUp"),
    ] {
        assert_eq!(serde_json::to_string(&action).unwrap(), format!("\"{want}\""));
        // 也就是 `payload` 里的拼写：`{"requestId":1,"cmd":"input","payload":{"action":"<want>"}}`。
        let cmd = Command::input(
            1,
            InputArgs { action, pointers: None, scroll: None, key: None },
        );
        assert_eq!(
            serde_json::to_string(&cmd).unwrap(),
            format!(r#"{{"requestId":1,"cmd":"input","payload":{{"action":"{want}"}}}}"#),
            "action 拼写变了：{want}"
        );
    }
    // 未知取值必须失败（协议版本精确匹配 ⇒ 取值集合不同就是对端版本不对）
    assert!(serde_json::from_str::<InputAction>("\"tap\"").is_err());
}

#[test]
fn kind_names_cover_all_nine_values() {
    for (kind, name) in [
        (kind::FRAME, "视频帧"),
        (kind::HELLO, "握手"),
        (kind::HELLO_ACK, "握手应答"),
        (kind::CAPS_REQUEST, "请求能力表"),
        (kind::CAPS, "能力表"),
        (kind::COMMAND, "指令"),
        (kind::COMMAND_ACK, "指令回报"),
        (kind::EVENT, "主动上报"),
        (kind::HEARTBEAT, "心跳"),
    ] {
        assert_eq!(kind_name(kind), name);
    }
    assert_eq!(kind_name(0x7F), "未知类型");
}
