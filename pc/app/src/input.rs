//! 输入上报：把窗口里的鼠标 / 键盘事件变成 `input` 指令。
//!
//! # 规则（`docs/spec/tech-stack/pc.md` §4，**必须做**）
//!
//! | 事件 | 上报方式 |
//! |---|---|
//! | `move` | **定频采样** —— 按上限频率取**最新坐标**，中间的直接丢 |
//! | `scroll` | **累加 `dx` / `dy`** 后按同一频率上报 —— 增量语义，**绝不能丢**（丢了就少滚） |
//! | `down` / `up` / `cancel` / `key` | **不合并、不限频、立即发**（合并会让点击丢或错位） |
//!
//! 频率上限 = **屏刷新率**（`applyScreen` 回报的 `refreshRate`）：屏才是"画面能体现的上限"，
//! 高于它的 `move` 对用户**没有任何可见价值**，只是把 JSON + CRC + socket 写的开销白付一遍。
//!
//! 本模块只**产出**指令；**发**由会话线程做 —— 协议规定「一条连接 = 一个写者」。
//!
//! # 单指模型
//!
//! PC 只有一个鼠标 ⇒ 只用**一个触点**（`id = 0`）：左键按下 = 触点按下、拖动 = 触点移动、
//! 松开 = 触点抬起。右键 = BACK、中键 = HOME（都是"按一下"的按键，不参与触点状态）。
//! 多点触控属 F11（后置）。

use std::time::{Duration, Instant};

use proto::{Command, InputAction, InputArgs, InputKey, InputPointer, InputScroll};

/// 拿不到 agent 回报的刷新率时的采样上限。
pub const DEFAULT_RATE_HZ: f32 = 60.0;

/// 单指模型的触点标识（手势内稳定 —— agent 由它推导 `pointerIndex`）。
const POINTER_ID: i32 = 0;

/// Android `metaState` 的位（只登记 PC 会产生的那些）。
pub mod meta {
    /// Shift 按下。
    pub const SHIFT: i32 = 0x1;
    /// Alt 按下。
    pub const ALT: i32 = 0x02;
    /// Ctrl 按下。
    pub const CTRL: i32 = 0x1000;
}

/// Android `keyCode` 常量（只登记本模块要用到的）。
///
/// 数值取自 `android.view.KeyEvent`；agent 侧**零映射**（协议直接传 keyCode）。
pub mod keycode {
    /// HOME。
    pub const HOME: i32 = 3;
    /// BACK。
    pub const BACK: i32 = 4;
    /// 数字 0。
    pub const NUM_0: i32 = 7;
    /// 数字 1。
    pub const NUM_1: i32 = 8;
    /// 数字 2。
    pub const NUM_2: i32 = 9;
    /// 数字 3。
    pub const NUM_3: i32 = 10;
    /// 数字 4。
    pub const NUM_4: i32 = 11;
    /// 数字 5。
    pub const NUM_5: i32 = 12;
    /// 数字 6。
    pub const NUM_6: i32 = 13;
    /// 数字 7。
    pub const NUM_7: i32 = 14;
    /// 数字 8。
    pub const NUM_8: i32 = 15;
    /// 数字 9。
    pub const NUM_9: i32 = 16;
    /// 方向键：上。
    pub const DPAD_UP: i32 = 19;
    /// 方向键：下。
    pub const DPAD_DOWN: i32 = 20;
    /// 方向键：左。
    pub const DPAD_LEFT: i32 = 21;
    /// 方向键：右。
    pub const DPAD_RIGHT: i32 = 22;
    /// 字母 A（字母表连续：A=29 … Z=54）。
    pub const A: i32 = 29;
    /// 逗号。
    pub const COMMA: i32 = 55;
    /// 句点。
    pub const PERIOD: i32 = 56;
    /// Tab。
    pub const TAB: i32 = 61;
    /// 空格。
    pub const SPACE: i32 = 62;
    /// 反引号。
    pub const GRAVE: i32 = 68;
    /// 减号。
    pub const MINUS: i32 = 69;
    /// 等号。
    pub const EQUALS: i32 = 70;
    /// 左方括号。
    pub const LEFT_BRACKET: i32 = 71;
    /// 右方括号。
    pub const RIGHT_BRACKET: i32 = 72;
    /// 反斜杠。
    pub const BACKSLASH: i32 = 73;
    /// 分号。
    pub const SEMICOLON: i32 = 74;
    /// 单引号。
    pub const APOSTROPHE: i32 = 75;
    /// 斜杠。
    pub const SLASH: i32 = 76;
    /// 回车。
    pub const ENTER: i32 = 66;
    /// 退格。
    pub const DEL: i32 = 67;
    /// 前向删除。
    pub const FORWARD_DEL: i32 = 112;
    /// PageUp。
    pub const PAGE_UP: i32 = 92;
    /// PageDown。
    pub const PAGE_DOWN: i32 = 93;
    /// Home（行首）。
    pub const MOVE_HOME: i32 = 122;
    /// End（行尾）。
    pub const MOVE_END: i32 = 123;
    /// Insert。
    pub const INSERT: i32 = 124;
    /// 功能键 F1（F1…F12 连续）。
    pub const F1: i32 = 131;
}

/// 一次采样周期内攒下的、需要**按上限频率**上报的状态。
///
/// 只被 UI 线程使用（生产）⇒ 不需要锁；产出的指令交给会话线程发送。
pub struct Sampler {
    /// 采样上限（Hz）—— = agent 回报的屏刷新率。
    rate_hz: f32,
    /// 上次按频率上报的时刻。
    last_flush: Instant,
    /// 最新光标位置（屏像素）；`None` = 光标还没进过画面。
    cursor: Option<(i32, i32)>,
    /// 左键是否按着（触点是否处于按下态）。
    dragging: bool,
    /// 自上次上报以来坐标是否变过。
    moved: bool,
    /// 累计的滚轮增量（**增量语义 ⇒ 只能累加**）。
    scroll_dx: i32,
    /// 累计的滚轮纵向增量。
    scroll_dy: i32,
    /// 滚轮发生的位置（屏像素）。
    scroll_at: Option<(i32, i32)>,
    /// 立即类事件（`down` / `up` / `cancel` / `key`）—— 保序，不合并。
    immediate: Vec<Command>,
}

impl Sampler {
    /// 建采样器。
    ///
    /// @param rate_hz 采样上限（Hz）；拿不到回报值时用 [`DEFAULT_RATE_HZ`]
    pub fn new(rate_hz: f32) -> Self {
        Self {
            rate_hz: sane_rate(rate_hz),
            last_flush: Instant::now(),
            cursor: None,
            dragging: false,
            moved: false,
            scroll_dx: 0,
            scroll_dy: 0,
            scroll_at: None,
            immediate: Vec::new(),
        }
    }

    /// 更新采样上限（收到 `applyScreen` 回报后调一次即可）。
    ///
    /// @param rate_hz 屏刷新率
    pub fn set_rate(&mut self, rate_hz: f32) {
        self.rate_hz = sane_rate(rate_hz);
    }

    /// 最新光标位置（屏像素）；`None` = 光标还没进过画面。
    pub fn cursor_pos(&self) -> Option<(i32, i32)> {
        self.cursor
    }

    /// 触点当前是否处于按下态（UI 用它判断"该不该补一个松开"）。
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// 光标移到画面内的某个屏像素位置。
    ///
    /// 只有**按着**（`dragging`）时这个位置才有意义 —— 悬停不产生触点事件。
    ///
    /// @param x 屏像素 x
    /// @param y 屏像素 y
    pub fn cursor(&mut self, x: i32, y: i32) {
        if self.cursor != Some((x, y)) {
            self.cursor = Some((x, y));
            self.moved = true;
        }
    }

    /// 左键按下 ⇒ **立即**产生触点按下。
    ///
    /// 光标还没进过画面时忽略（没有坐标就无从按下）。
    pub fn press(&mut self) {
        let Some((x, y)) = self.cursor else {
            return;
        };
        self.dragging = true;
        self.moved = false;
        self.immediate.push(touch(InputAction::Down, &[(x, y)]));
    }

    /// 左键抬起 ⇒ **立即**产生触点抬起（`pointers` 为空 = 抬起后没有剩下的触点）。
    pub fn release(&mut self) {
        if !self.dragging {
            return;
        }
        self.dragging = false;
        self.moved = false;
        self.immediate.push(touch(InputAction::Up, &[]));
    }

    /// 手势取消 —— 窗口失焦 / 指针离开画面时调。
    ///
    /// 必须发：不发的话 agent 侧那个触点**永远按着**，之后所有点击都会被当成拖动 ✗
    pub fn cancel(&mut self) {
        if !self.dragging {
            return;
        }
        self.dragging = false;
        self.moved = false;
        self.immediate.push(touch(InputAction::Cancel, &[]));
    }

    /// 滚轮 —— **累加**，等下一次按频率上报。
    ///
    /// @param dx 横向增量
    /// @param dy 纵向增量
    /// @param x 位置 x（屏像素）
    /// @param y 位置 y（屏像素）
    pub fn scroll(&mut self, dx: i32, dy: i32, x: i32, y: i32) {
        self.scroll_dx = self.scroll_dx.saturating_add(dx);
        self.scroll_dy = self.scroll_dy.saturating_add(dy);
        self.scroll_at = Some((x, y));
    }

    /// 按键 ⇒ **立即**产生 `keyDown` / `keyUp`（不合并、不限频）。
    ///
    /// @param code Android keyCode
    /// @param meta metaState
    /// @param pressed true = 按下
    pub fn key(&mut self, code: i32, meta: i32, pressed: bool) {
        self.immediate.push(Command::input(
            crate::session::next_request_id(),
            InputArgs {
                action: if pressed { InputAction::KeyDown } else { InputAction::KeyUp },
                pointers: None,
                scroll: None,
                key: Some(InputKey { code, meta }),
            },
        ));
    }

    /// 取出这一轮要发的全部指令。
    ///
    /// 立即类事件**先出**（保证 `down` 排在它自己的 `move` 之前）；随后是到点的
    /// 定频上报（`move` / `scroll`）。UI 每帧调一次即可。
    ///
    /// @param now 当前时刻（注入以便单测）
    /// @return 待发指令（可能为空）
    pub fn poll(&mut self, now: Instant) -> Vec<Command> {
        let mut out = std::mem::take(&mut self.immediate);

        let interval = Duration::from_secs_f32(1.0 / self.rate_hz);
        if now.duration_since(self.last_flush) >= interval {
            self.last_flush = now;
            if self.dragging && self.moved {
                if let Some((x, y)) = self.cursor {
                    out.push(touch(InputAction::Move, &[(x, y)]));
                    self.moved = false;
                }
            }
            if self.scroll_dx != 0 || self.scroll_dy != 0 {
                let (x, y) = self.scroll_at.unwrap_or((0, 0));
                out.push(Command::input(
                    crate::session::next_request_id(),
                    InputArgs {
                        action: InputAction::Scroll,
                        pointers: None,
                        scroll: Some(InputScroll { dx: self.scroll_dx, dy: self.scroll_dy, x, y }),
                        key: None,
                    },
                ));
                self.scroll_dx = 0;
                self.scroll_dy = 0;
            }
        }
        out
    }
}

/// 造一条触摸指令。
///
/// @param action `down` / `move` / `up` / `cancel`
/// @param points 触点快照（**变更后**的当前触点集合）
/// @return 指令
fn touch(action: InputAction, points: &[(i32, i32)]) -> Command {
    Command::input(
        crate::session::next_request_id(),
        InputArgs {
            action,
            pointers: Some(
                points
                    .iter()
                    .map(|&(x, y)| InputPointer { id: POINTER_ID, x, y })
                    .collect(),
            ),
            scroll: None,
            key: None,
        },
    )
}

/// 把不在合理区间的刷新率拉回默认值 —— 0 会让采样变成"每帧都发"，负数会 panic。
///
/// @param hz 请求的速率
/// @return 可用的速率
fn sane_rate(hz: f32) -> f32 {
    if hz.is_finite() && (1.0..=500.0).contains(&hz) {
        hz
    } else {
        DEFAULT_RATE_HZ
    }
}

/// egui 按键 → Android `keyCode`。
///
/// 返回 `None` = 这个键不映射（不认识的键**静默丢弃**，绝不去猜 —— 猜错会往屏上打出一个
/// 用户没按的键）。
///
/// @param key egui 的按键
/// @return Android keyCode，或 `None`
pub fn android_keycode(key: egui::Key) -> Option<i32> {
    use egui::Key;
    use keycode as kc;
    Some(match key {
        Key::ArrowDown => kc::DPAD_DOWN,
        Key::ArrowLeft => kc::DPAD_LEFT,
        Key::ArrowRight => kc::DPAD_RIGHT,
        Key::ArrowUp => kc::DPAD_UP,
        Key::Escape => kc::BACK,
        Key::Tab => kc::TAB,
        Key::Backspace => kc::DEL,
        Key::Enter => kc::ENTER,
        Key::Space => kc::SPACE,
        Key::Insert => kc::INSERT,
        Key::Delete => kc::FORWARD_DEL,
        Key::Home => kc::MOVE_HOME,
        Key::End => kc::MOVE_END,
        Key::PageUp => kc::PAGE_UP,
        Key::PageDown => kc::PAGE_DOWN,
        Key::Comma => kc::COMMA,
        Key::Period => kc::PERIOD,
        Key::Minus => kc::MINUS,
        Key::Plus | Key::Equals => kc::EQUALS,
        Key::Semicolon | Key::Colon => kc::SEMICOLON,
        Key::Backslash | Key::Pipe => kc::BACKSLASH,
        Key::Slash | Key::Questionmark => kc::SLASH,
        Key::OpenBracket | Key::OpenCurlyBracket => kc::LEFT_BRACKET,
        Key::CloseBracket | Key::CloseCurlyBracket => kc::RIGHT_BRACKET,
        Key::Backtick => kc::GRAVE,
        Key::Quote => kc::APOSTROPHE,
        Key::Num0 => kc::NUM_0,
        Key::Num1 => kc::NUM_1,
        Key::Num2 => kc::NUM_2,
        Key::Num3 => kc::NUM_3,
        Key::Num4 => kc::NUM_4,
        Key::Num5 => kc::NUM_5,
        Key::Num6 => kc::NUM_6,
        Key::Num7 => kc::NUM_7,
        Key::Num8 => kc::NUM_8,
        Key::Num9 => kc::NUM_9,
        Key::F1 => kc::F1,
        Key::F2 => kc::F1 + 1,
        Key::F3 => kc::F1 + 2,
        Key::F4 => kc::F1 + 3,
        Key::F5 => kc::F1 + 4,
        Key::F6 => kc::F1 + 5,
        Key::F7 => kc::F1 + 6,
        Key::F8 => kc::F1 + 7,
        Key::F9 => kc::F1 + 8,
        Key::F10 => kc::F1 + 9,
        Key::F11 => kc::F1 + 10,
        Key::F12 => kc::F1 + 11,
        // 字母表在 Android 里是连续的（A=29 … Z=54），egui 的枚举顺序也是 A…Z ⇒ 用偏移。
        other => {
            let idx = letter_index(other)?;
            kc::A + idx
        }
    })
}

/// 字母键在字母表里的下标。
///
/// @param key egui 的按键
/// @return `A` = 0 … `Z` = 25；不是字母时 `None`
fn letter_index(key: egui::Key) -> Option<i32> {
    use egui::Key;
    const LETTERS: [Key; 26] = [
        Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J, Key::K,
        Key::L, Key::M, Key::N, Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T, Key::U, Key::V,
        Key::W, Key::X, Key::Y, Key::Z,
    ];
    LETTERS.iter().position(|&k| k == key).map(|i| i as i32)
}

/// egui 修饰键 → Android `metaState`。
///
/// Windows 上 `command` 与 `ctrl` 是同一个物理键（egui 两者都置位）⇒ **只认 `ctrl`**，
/// 免得 Alt 之外又多出一份 Meta ✗
///
/// @param m egui 的修饰键状态
/// @return metaState 位组合
pub fn android_meta(m: &egui::Modifiers) -> i32 {
    let mut out = 0;
    if m.shift {
        out |= meta::SHIFT;
    }
    if m.alt {
        out |= meta::ALT;
    }
    if m.ctrl {
        out |= meta::CTRL;
    }
    out
}

#[cfg(test)]
mod tests {
    //! 采样与映射的单测。
    //!
    //! 这几条规则错了**只有在真机上才看得出来**（少滚 / 点击错位 / 丢事件），
    //! 而它们在正常的桌面操作里都不明显 ⇒ 必须在单测里钉死。

    use super::*;
    use proto::CommandBody;

    /// 造一个采样器（固定 60Hz ⇒ 16.67ms 一个周期）。
    fn sampler() -> Sampler {
        Sampler::new(60.0)
    }

    /// 取出指令的输入载荷（不是 `input` 指令就 panic）。
    fn args_of(cmd: &Command) -> &InputArgs {
        match &cmd.body {
            CommandBody::Input(args) => args,
            other => panic!("不是输入指令：{other:?}"),
        }
    }

    /// 取出指令的 (action, request_id)。
    fn action_of(cmd: &Command) -> (InputAction, i32) {
        (args_of(cmd).action, cmd.request_id)
    }

    #[test]
    fn 移动被合并成一条() {
        let t0 = Instant::now();
        let mut s = sampler();
        s.cursor(10, 10);
        s.press();
        drop(s.poll(t0)); // 立即类：down
        s.cursor(11, 11);
        s.cursor(12, 12);
        s.cursor(13, 13);
        let out = s.poll(t0 + Duration::from_millis(20));
        assert_eq!(out.len(), 1, "三次移动只应产生一条 move");
        assert_eq!(action_of(&out[0]).0, InputAction::Move);
        match &args_of(&out[0]).pointers {
            Some(p) => assert_eq!((p[0].x, p[0].y), (13, 13)),
            None => panic!("move 必须带 pointers"),
        }
    }

    #[test]
    fn 未到采样周期不发移动() {
        let t0 = Instant::now();
        let mut s = sampler();
        s.cursor(1, 1);
        s.press();
        s.poll(t0);
        s.cursor(2, 2);
        assert!(s.poll(t0 + Duration::from_millis(5)).is_empty(), "5ms < 16.7ms");
        assert_eq!(s.poll(t0 + Duration::from_millis(20)).len(), 1);
    }

    #[test]
    fn 悬停不发移动() {
        let t0 = Instant::now();
        let mut s = sampler();
        s.cursor(1, 1);
        s.cursor(2, 2);
        // 没按下 ⇒ 没有触点，悬停不该产生任何事件
        assert!(s.poll(t0 + Duration::from_millis(20)).is_empty());
    }

    #[test]
    fn 滚轮累加不丢() {
        let t0 = Instant::now();
        let mut s = sampler();
        s.scroll(0, -120, 100, 200);
        s.scroll(0, -120, 100, 200);
        s.scroll(5, -120, 101, 201);
        let out = s.poll(t0 + Duration::from_millis(20));
        assert_eq!(out.len(), 1);
        let args = args_of(&out[0]);
        match &args.scroll {
            Some(sc) => {
                assert_eq!(args.action, InputAction::Scroll);
                assert_eq!((sc.dx, sc.dy), (5, -360), "增量必须相加");
                assert_eq!((sc.x, sc.y), (101, 201), "位置取最新");
            }
            None => panic!("scroll 必须带 scroll"),
        }
        // 上报后清零
        assert!(s.poll(t0 + Duration::from_millis(40)).is_empty());
    }

    #[test]
    fn 按下与抬起立即发且成对() {
        let t0 = Instant::now();
        let mut s = sampler();
        s.cursor(50, 60);
        s.press();
        s.release();
        let out = s.poll(t0); // 远没到采样周期
        assert_eq!(out.len(), 2);
        assert_eq!(action_of(&out[0]).0, InputAction::Down);
        assert_eq!(action_of(&out[1]).0, InputAction::Up);
        match &args_of(&out[0]).pointers {
            Some(p) => assert_eq!((p[0].id, p[0].x, p[0].y), (0, 50, 60)),
            None => panic!("down 必须带 pointers"),
        }
        match &args_of(&out[1]).pointers {
            Some(p) => assert!(p.is_empty(), "抬起后没有剩下的触点"),
            None => panic!("up 必须带 pointers"),
        }
    }

    #[test]
    fn 没有光标时按下被忽略() {
        let mut s = sampler();
        s.press();
        assert!(s.poll(Instant::now()).is_empty());
    }

    #[test]
    fn 失焦取消手势() {
        let t0 = Instant::now();
        let mut s = sampler();
        s.cursor(1, 1);
        s.press();
        s.poll(t0);
        s.cancel();
        let out = s.poll(t0 + Duration::from_millis(20));
        assert_eq!(out.len(), 1);
        assert_eq!(action_of(&out[0]).0, InputAction::Cancel);
        // 取消后再动鼠标不该再发移动（触点已经不在）
        s.cursor(9, 9);
        assert!(s.poll(t0 + Duration::from_millis(40)).is_empty());
    }

    #[test]
    fn 按键成对且带修饰位() {
        let t0 = Instant::now();
        let mut s = sampler();
        s.key(keycode::A, meta::CTRL, true);
        s.key(keycode::A, meta::CTRL, false);
        let out = s.poll(t0);
        assert_eq!(out.len(), 2);
        let (a, b) = (args_of(&out[0]), args_of(&out[1]));
        match (&a.key, &b.key) {
            (Some(k1), Some(k2)) => {
                assert_eq!((a.action, b.action), (InputAction::KeyDown, InputAction::KeyUp));
                assert_eq!(k1.code, keycode::A);
                assert_eq!(k1.meta, meta::CTRL);
                assert_eq!(k2.code, keycode::A);
            }
            other => panic!("不是按键指令：{other:?}"),
        }
    }

    #[test]
    fn 请求号单调递增() {
        let mut s = sampler();
        s.key(1, 0, true);
        let a = action_of(&s.poll(Instant::now())[0]).1;
        s.key(1, 0, true);
        let b = action_of(&s.poll(Instant::now())[0]).1;
        assert!(b > a, "{b} 应大于 {a}");
    }

    #[test]
    fn 字母与数字映射到正确的安卓编码() {
        assert_eq!(android_keycode(egui::Key::A), Some(29));
        assert_eq!(android_keycode(egui::Key::Z), Some(54));
        assert_eq!(android_keycode(egui::Key::M), Some(41));
        assert_eq!(android_keycode(egui::Key::Num0), Some(7));
        assert_eq!(android_keycode(egui::Key::Num9), Some(16));
        assert_eq!(android_keycode(egui::Key::Enter), Some(66));
        assert_eq!(android_keycode(egui::Key::Backspace), Some(67));
        assert_eq!(android_keycode(egui::Key::F12), Some(142));
    }

    #[test]
    fn 刷新率异常时回落默认值() {
        assert_eq!(sane_rate(0.0), DEFAULT_RATE_HZ);
        assert_eq!(sane_rate(-1.0), DEFAULT_RATE_HZ);
        assert_eq!(sane_rate(f32::NAN), DEFAULT_RATE_HZ);
        assert_eq!(sane_rate(90.0), 90.0);
    }
}
