use crate::{
    driver::uart::uart::{c_uart_send, c_uart_send_str, UartPort},
    include::bindings::bindings::{font_ascii, video_frame_buffer_info},
    kBUG, kinfo,
    libs::spinlock::SpinLock,
    syscall::SystemError,
};
use alloc::{boxed::Box, collections::LinkedList, string::ToString};
use alloc::{sync::Arc, vec::Vec};
use core::{
    fmt::Debug,
    intrinsics::unlikely,
    ops::{Add, AddAssign, Deref, DerefMut, Sub},
    ptr::copy_nonoverlapping,
    sync::atomic::{AtomicI32, AtomicU32, Ordering},
};
use thingbuf::mpsc;

use super::{
    screen_manager::{
        scm_register, ScmBufferInfo, ScmFramworkType, ScmUiFramework, ScmUiFrameworkMetadata,
    },
    textui_no_alloc::no_init_textui_putchar_window,
};
use lazy_static::lazy_static;

lazy_static! {
    pub static ref WINDOW_MPSC: WindowMpsc = WindowMpsc::new();
}
/// 声明全局的TEXTUI_FRAMEWORK
pub static mut TEXTUI_FRAMEWORK: Option<Box<TextUiFramework>> = None;
/// 获取TEXTUI_FRAMEWORK的可变实例
pub fn textui_framework() -> &'static mut TextUiFramework {
    return unsafe { TEXTUI_FRAMEWORK.as_mut().unwrap() };
}
/// 初始化TEXTUI_FRAMEWORK
pub unsafe fn textui_framwork_init() {
    if TEXTUI_FRAMEWORK.is_none() {
        kinfo!("textuiframework init");
        TEXTUI_FRAMEWORK = Some(Box::new(TextUiFramework::new(
            Arc::new(SpinLock::new(TextuiWindow::new(
                WindowFlag::TEXTUI_IS_CHROMATIC,
                0,
                0,
            ))),
            Arc::new(SpinLock::new(TextuiWindow::new(
                WindowFlag::TEXTUI_IS_CHROMATIC,
                0,
                0,
            ))),
        )));
    } else {
        kBUG!("Try to init TEXTUI_FRAMEWORK twice!");
    }
}
// window标志位
bitflags! {
    pub struct WindowFlag: u8 {
        // 采用彩色字符
        const TEXTUI_IS_CHROMATIC = 1 << 0;
    }
}

/// 每个字符的宽度和高度（像素）
pub const TEXTUI_CHAR_WIDTH: u32 = 8;

pub const TEXTUI_CHAR_HEIGHT: u32 = 16;

pub static mut TEST_IS_INIT: bool = false;

/// 因为只在未初始化textui之前而其他模块使用的内存将要到达48M时到在初始化textui时才为false,所以只会修改两次，应该不需加锁
pub static mut ENABLE_PUT_TO_WINDOW: bool = true; 

/// 利用mpsc实现当前窗口

pub struct WindowMpsc {
    receiver: mpsc::Receiver<TextuiWindow>,
    sender: mpsc::Sender<TextuiWindow>,
}

impl WindowMpsc {
    pub const MPSC_BUF_SIZE: usize = 512;
    fn new() -> Self {
        // let window = &TEXTUI_PRIVATE_INFO.lock().current_window;
        let (sender, receiver) = mpsc::channel::<TextuiWindow>(Self::MPSC_BUF_SIZE);
        WindowMpsc { receiver, sender }
    }
}

/**
 * @brief 黑白字符对象
 *
 */
#[derive(Clone, Debug)]
struct TextuiCharNormal {
    _data: u8,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Default)]
pub struct LineId(i32);
impl LineId {
    pub fn new(num: i32) -> Self {
        LineId(num)
    }

    pub fn check(&self, max: i32) -> bool {
        self.0 < max && self.0 >= 0
    }

    pub fn data(&self) -> i32 {
        self.0
    }
}
impl Add<i32> for LineId {
    type Output = LineId;
    fn add(self, rhs: i32) -> Self::Output {
        LineId::new(self.0 + rhs)
    }
}
impl Sub<i32> for LineId {
    type Output = LineId;

    fn sub(self, rhs: i32) -> Self::Output {
        LineId::new(self.0 - rhs)
    }
}

impl Into<i32> for LineId {
    fn into(self) -> i32 {
        self.0.clone()
    }
}
impl Into<u32> for LineId {
    fn into(self) -> u32 {
        self.0.clone() as u32
    }
}
impl Into<usize> for LineId {
    fn into(self) -> usize {
        self.0.clone() as usize
    }
}
impl Sub<LineId> for LineId {
    type Output = LineId;

    fn sub(mut self, rhs: LineId) -> Self::Output {
        self.0 -= rhs.0;
        return self;
    }
}
impl AddAssign<LineId> for LineId {
    fn add_assign(&mut self, rhs: LineId) {
        self.0 += rhs.0;
    }
}
#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Default)]
pub struct LineIndex(i32);
impl LineIndex {
    pub fn new(num: i32) -> Self {
        LineIndex(num)
    }
    pub fn check(&self, chars_per_line: i32) -> bool {
        self.0 < chars_per_line && self.0 >= 0
    }
}
impl Add<LineIndex> for LineIndex {
    type Output = LineIndex;

    fn add(self, rhs: LineIndex) -> Self::Output {
        LineIndex::new(self.0 + rhs.0)
    }
}
impl Add<i32> for LineIndex {
    // type Output = Self;
    type Output = LineIndex;

    fn add(self, rhs: i32) -> Self::Output {
        LineIndex::new(self.0 + rhs)
    }
}
impl Sub<i32> for LineIndex {
    type Output = LineIndex;

    fn sub(self, rhs: i32) -> Self::Output {
        LineIndex::new(self.0 - rhs)
    }
}

impl Into<i32> for LineIndex {
    fn into(self) -> i32 {
        self.0.clone()
    }
}
impl Into<u32> for LineIndex {
    fn into(self) -> u32 {
        self.0.clone() as u32
    }
}
impl Into<usize> for LineIndex {
    fn into(self) -> usize {
        self.0.clone() as usize
    }
}
#[derive(Copy, Clone, Debug)]
pub struct FontColor(u32);
#[allow(dead_code)]
impl FontColor {
    pub const BLUE: FontColor = FontColor::new(0, 0, 0xff);
    pub const RED: FontColor = FontColor::new(0xff, 0, 0);
    pub const GREEN: FontColor = FontColor::new(0, 0xff, 0);
    pub const WHITE: FontColor = FontColor::new(0xff, 0xff, 0xff);
    pub const BLACK: FontColor = FontColor::new(0, 0, 0);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        let val = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        return FontColor(val & 0x00ffffff);
    }
}

impl From<u32> for FontColor {
    fn from(value: u32) -> Self {
        return Self(value & 0x00ffffff);
    }
}
impl Into<usize> for FontColor {
    fn into(self) -> usize {
        self.0.clone() as usize
    }
}
impl Into<u32> for FontColor {
    fn into(self) -> u32 {
        self.0.clone()
    }
}
impl Into<u16> for FontColor {
    fn into(self) -> u16 {
        self.0.clone() as u16
    }
}
impl Into<u64> for FontColor {
    fn into(self) -> u64 {
        self.0.clone() as u64
    }
}

/// 彩色字符对象

#[derive(Clone, Debug, Copy)]
pub struct TextuiCharChromatic {
    c: u8,

    // 前景色
    frcolor: FontColor, // rgb

    // 背景色
    bkcolor: FontColor, // rgb
}

// pub fn set_textui_buf_vaddr(vaddr: usize) {
//     // *TEXTUI_BUF_VADDR.write() = vaddr;
//     TEXTUI_BUF_VADDR.store(vaddr, Ordering::SeqCst);
// }
// pub fn set_textui_buf_size(size: usize) {
//     // *TEXTUI_BUF_SIZE.write() = size;
//     TEXTUI_BUF_SIZE.store(size, Ordering::SeqCst);
// }
// pub fn set_textui_buf_width(width: u32) {
//     // *TEXTUI_BUF_WIDTH.write() = width;
//     TEXTUI_BUF_WIDTH.store(width, Ordering::SeqCst);
// }
#[derive(Debug)]
pub struct TextuiBuf<'a>(&'a mut [u32]);

impl TextuiBuf<'_> {
    pub fn new(buf: &mut [u32]) -> TextuiBuf {
        TextuiBuf(buf)
    }
    pub fn get_buf_from_vaddr(vaddr: usize, len: usize) -> TextuiBuf<'static> {
        let new_buf: &mut [u32] =
            unsafe { core::slice::from_raw_parts_mut(vaddr as *mut u32, len) };
        let buf: TextuiBuf<'_> = TextuiBuf::new(new_buf);
        return buf;
    }

    pub fn put_color_in_pixel(&mut self, color: u32, index: usize) {
        let buf: &mut [u32] = self.0;
        buf[index] = color;
    }
    pub fn get_index_of_next_line(now_index: usize) -> usize {
        // *(TEXTUI_BUF_WIDTH.read()) as usize + now_index
        // TEXTUI_BUF_WIDTH.load(Ordering::SeqCst) as usize + now_index
        textui_framework().metadata.buf_info.buf_width() as usize + now_index
    }
    pub fn get_index_by_x_y(x: usize, y: usize) -> usize {
        // *(TEXTUI_BUF_WIDTH.read()) as usize * y + x
        textui_framework().metadata.buf_info.buf_width() as usize * y + x
    }
    pub fn get_start_index_by_lineid_lineindex(lineid: LineId, lineindex: LineIndex) -> usize {
        //   x 左上角列像素点位置
        //   y 左上角行像素点位置
        let index_x: u32 = lineindex.into();
        let x: u32 = index_x * TEXTUI_CHAR_WIDTH;

        let id_y: u32 = lineid.into();
        let y: u32 = id_y * TEXTUI_CHAR_HEIGHT;

        TextuiBuf::get_index_by_x_y(x as usize, y as usize)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct Font([u8; 16]);
impl Font {
    pub fn get_font(index: usize) -> Font {
        Self(unsafe { font_ascii[index] })
    }
    pub fn is_frcolor(&self, height: usize, width: usize) -> bool {
        let w = self.0[height];
        let testbit = 1 << (8 - width);
        w & testbit != 0
    }
}

impl TextuiCharChromatic {
    pub fn new(c: u8, frcolor: FontColor, bkcolor: FontColor) -> Self {
        TextuiCharChromatic {
            c,
            frcolor,
            bkcolor,
        }
    }

    /// 将该字符对象输出到缓冲区
    /// ## 参数
    /// -line_id 要放入的真实行号
    /// -index 要放入的真实列号
    pub fn textui_refresh_character(
        &self,
        lineid: LineId,
        lineindex: LineIndex,
    ) -> Result<i32, SystemError> {
        // 找到要渲染的字符的像素点数据
        let font: Font = Font::get_font(self.c as usize);

        let mut count = TextuiBuf::get_start_index_by_lineid_lineindex(lineid, lineindex);

        // let buf=TEXTUI_BUF.lock();
        // let vaddr = *TEXTUI_BUF_VADDR.read();
        // let vaddr = TEXTUI_BUF_VADDR.load(Ordering::SeqCst);
        let vaddr = textui_framework().metadata.buf_info.vaddr();
        // let len = *TEXTUI_BUF_SIZE.read();
        // let len = TEXTUI_BUF_SIZE.load(Ordering::SeqCst);
        let len = textui_framework().metadata.buf_info.buf_size_about_u32() as usize;
        let mut buf = TextuiBuf::get_buf_from_vaddr(vaddr, len);
        // 在缓冲区画出一个字体，每个字体有TEXTUI_CHAR_HEIGHT行，TEXTUI_CHAR_WIDTH列个像素点
        for i in 0..TEXTUI_CHAR_HEIGHT {
            let start = count;
            for j in 0..TEXTUI_CHAR_WIDTH {
                if font.is_frcolor(i as usize, j as usize) {
                    // 字，显示前景色
                    buf.put_color_in_pixel(self.frcolor.into(), count);
                } else {
                    // 背景色
                    buf.put_color_in_pixel(self.bkcolor.into(), count);
                }
                count += 1;
            }
            count = TextuiBuf::get_index_of_next_line(start);
        }
        return Ok(0);
    }

    pub fn no_init_textui_render_chromatic(&self, lineid: LineId, lineindex: LineIndex) {
        // 找到要渲染的字符的像素点数据
        let font = unsafe { font_ascii }[self.c as usize];

        //   x 左上角列像素点位置
        //   y 左上角行像素点位置
        let index_x: u32 = lineindex.into();
        let x: u32 = index_x * TEXTUI_CHAR_WIDTH;

        let id_y: u32 = lineid.into();
        let y: u32 = id_y * TEXTUI_CHAR_HEIGHT;
        // 找到输入缓冲区的起始地址位置
        let fb = unsafe { video_frame_buffer_info.vaddr };

        let mut testbit: u32; // 用来测试特定行的某列是背景还是字体本身

        // 在缓冲区画出一个字体，每个字体有TEXTUI_CHAR_HEIGHT行，TEXTUI_CHAR_WIDTH列个像素点
        for i in 0..TEXTUI_CHAR_HEIGHT {
            // 计算出帧缓冲区每一行打印的起始位置的地址（起始位置+（y+i）*缓冲区的宽度+x）

            let mut addr: *mut u32 = (fb as u32
                + unsafe { video_frame_buffer_info.width } * 4 * (y as u32 + i)
                + 4 * x as u32) as *mut u32;

            testbit = 1 << (TEXTUI_CHAR_WIDTH + 1);
            for _j in 0..TEXTUI_CHAR_WIDTH {
                //从左往右逐个测试相应位
                testbit >>= 1;
                if (font[i as usize] & testbit as u8) != 0 {
                    unsafe { *addr = self.frcolor.into() }; // 字，显示前景色
                } else {
                    unsafe { *addr = self.bkcolor.into() }; // 背景色
                }

                unsafe {
                    addr = (addr.offset(1)) as *mut u32;
                }
            }
        }
    }
}

/// 单色显示的虚拟行结构体

#[derive(Clone, Debug, Default)]
pub struct TextuiVlineNormal {
    _characters: Vec<TextuiCharNormal>, // 字符对象数组
    _index: i16,                        // 当前操作的位置
}
/// 彩色显示的虚拟行结构体

#[derive(Clone, Debug, Default)]
pub struct TextuiVlineChromatic {
    chars: Vec<TextuiCharChromatic>, // 字符对象数组
    index: LineIndex,                // 当前操作的位置
}
impl TextuiVlineChromatic {
    pub fn new(char_num: usize) -> Self {
        let mut r = TextuiVlineChromatic {
            chars: Vec::with_capacity(char_num),
            index: LineIndex::new(0),
        };

        for _ in 0..char_num {
            r.chars.push(TextuiCharChromatic::new(
                0,
                FontColor::BLACK,
                FontColor::BLACK,
            ));
        }

        return r;
    }
}

#[derive(Clone, Debug)]
pub enum TextuiVline {
    Chromatic(TextuiVlineChromatic),
    _Normal(TextuiVlineNormal),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct WindowId(u32);

impl WindowId {
    pub fn new() -> Self {
        static MAX_ID: AtomicU32 = AtomicU32::new(0);
        return WindowId(MAX_ID.fetch_add(1, Ordering::SeqCst));
    }
}
#[derive(Clone, Debug)]
pub struct TextuiWindow {
    // 虚拟行是个循环表，头和尾相接
    id: WindowId,
    // 虚拟行总数
    vline_sum: i32,
    // 当前已经使用了的虚拟行总数（即在已经输入到缓冲区（之后显示在屏幕上）的虚拟行数量）
    vlines_used: i32,
    // 位于最顶上的那一个虚拟行的行号
    top_vline: LineId,
    // 储存虚拟行的数组
    vlines: Vec<TextuiVline>,
    // 正在操作的vline
    vline_operating: LineId,
    // 每行最大容纳的字符数
    chars_per_line: i32,
    // 窗口flag
    flags: WindowFlag,
}

impl TextuiWindow {
    /// 使用参数初始化window对象
    /// ## 参数
    ///
    /// -flags 标志位
    /// -vlines_num 虚拟行的总数
    /// -chars_num 每行最大的字符数
    
    pub fn new(flags: WindowFlag, vlines_num: i32, chars_num: i32) -> Self {
        let mut initial_vlines = Vec::new();

        for _ in 0..vlines_num {
            let vline = TextuiVlineChromatic::new(chars_num as usize);

            initial_vlines.push(TextuiVline::Chromatic(vline));
        }
        TextuiWindow {
            id: WindowId::new(),
            flags,
            vline_sum: vlines_num,
            vlines_used: 1,
            top_vline: LineId::new(0),
            vlines: initial_vlines,
            vline_operating: LineId::new(0),
            chars_per_line: chars_num,
        }
    }


    /// 刷新某个窗口的缓冲区的某个虚拟行的连续n个字符对象
    /// ## 参数
    /// - window 窗口结构体
    /// - vline_id 要刷新的虚拟行号
    /// - start 起始字符号
    /// - count 要刷新的字符数量

    fn textui_refresh_characters(
        &mut self,
        vline_id: LineId,
        start: LineIndex,
        count: i32,
    ) -> Result<i32, SystemError> {

        let actual_line_sum = textui_framework().actual_line.load(Ordering::SeqCst);

        // 判断虚拟行参数是否合法
        if unlikely(
            !vline_id.check(self.vline_sum)
                || (<LineIndex as Into<i32>>::into(start) + count) > self.chars_per_line,
        ) {
            return Err(SystemError::EINVAL);
        }
        // 计算虚拟行对应的真实行（即要渲染的行）
        let mut actual_line_id = vline_id - self.top_vline; //为正说明虚拟行不在真实行显示的区域上面

        if <LineId as Into<i32>>::into(actual_line_id) < 0 {
            //真实行数小于虚拟行数，则需要加上真实行数的位置，以便正确计算真实行
            actual_line_id = actual_line_id + actual_line_sum;
        }

        // 将此窗口的某个虚拟行的连续n个字符对象往缓存区写入
        if self.flags.contains(WindowFlag::TEXTUI_IS_CHROMATIC) {
            let vline = &mut self.vlines[<LineId as Into<usize>>::into(vline_id)];
            let mut i = 0;
            let mut index = start;

            while i < count {
                if let TextuiVline::Chromatic(vline) = vline {
                    vline.chars[<LineIndex as Into<usize>>::into(index)]
                        .textui_refresh_character(actual_line_id, index)?;

                    index = index + 1;
                }
                i += 1;
            }
        }

        return Ok(0);
    }

    /// 重新渲染某个窗口的某个虚拟行
    /// ## 参数

    /// - window 窗口结构体
    /// - vline_id 虚拟行号

    fn textui_refresh_vline(&mut self, vline_id: LineId) -> Result<i32, SystemError> {
        if self.flags.contains(WindowFlag::TEXTUI_IS_CHROMATIC) {
            return self.textui_refresh_characters(
                vline_id,
                LineIndex::new(0),
                self.chars_per_line,
            );
        } else {
            //todo支持纯文本字符
            todo!();
        }
    }

    // 刷新某个窗口的start 到start + count行（即将这些行输入到缓冲区）
    fn textui_refresh_vlines(&mut self, start: LineId, count: i32) -> Result<i32, SystemError> {
        let mut refresh_count = count;
        for i in <LineId as Into<i32>>::into(start)
            ..(self.vline_sum).min(<LineId as Into<i32>>::into(start) + count)
        {
            self.textui_refresh_vline(LineId::new(i))?;
            refresh_count -= 1;
        }
        //因为虚拟行是循环表
        let mut refresh_start = 0;
        while refresh_count > 0 {
            self.textui_refresh_vline(LineId::new(refresh_start))?;
            refresh_start += 1;
            refresh_count -= 1;
        }
        return Ok(0);
    }

    /// 往某个窗口的缓冲区的某个虚拟行插入换行
    /// ## 参数
    /// - window 窗口结构体
    /// - vline_id 虚拟行号

    fn textui_new_line(&mut self) -> Result<i32, SystemError> {
        // todo: 支持在两个虚拟行之间插入一个新行
        let actual_line_sum = textui_framework().actual_line.load(Ordering::SeqCst);
        self.vline_operating = self.vline_operating + 1;
        //如果已经到了最大行数，则重新从0开始
        if !self.vline_operating.check(self.vline_sum) {
            self.vline_operating = LineId::new(0);
        }

        if let TextuiVline::Chromatic(vline) =
            &mut (self.vlines[<LineId as Into<usize>>::into(self.vline_operating)])
        {
            for i in 0..self.chars_per_line {
                if let Some(v_char) = vline.chars.get_mut(i as usize) {
                    v_char.c = 0;
                    v_char.frcolor = FontColor::BLACK;
                    v_char.bkcolor = FontColor::BLACK;
                }
            }
            vline.index = LineIndex::new(0);
        }
        // 当已经使用的虚拟行总数等于真实行总数时，说明窗口中已经显示的文本行数已经达到了窗口的最大容量。这时，如果继续在窗口中添加新的文本，就会导致文本溢出窗口而无法显示。因此，需要往下滚动屏幕来显示更多的文本。

        if self.vlines_used == actual_line_sum {
            self.top_vline = self.top_vline + 1;

            if !self.top_vline.check(self.vline_sum) {
                self.top_vline = LineId::new(0);
            }

            // 刷新所有行
            self.textui_refresh_vlines(self.top_vline, actual_line_sum)?;
        } else {
            //换行说明上一行已经在缓冲区中，所以已经使用的虚拟行总数+1
            self.vlines_used += 1;
        }

        return Ok(0);
    }

    /// 真正向窗口的缓冲区上输入字符的函数(位置为window.vline_operating，window.vline_operating.index)
    /// ## 参数
    /// - window
    /// - character

    fn true_textui_putchar_window(
        &mut self,
        character: u8,
        frcolor: FontColor,
        bkcolor: FontColor,
    ) -> Result<i32, SystemError> {
        // 启用彩色字符
        if self.flags.contains(WindowFlag::TEXTUI_IS_CHROMATIC) {
            let mut line_index = LineIndex::new(0); //操作的列号
            if let TextuiVline::Chromatic(vline) =
                &mut (self.vlines[<LineId as Into<usize>>::into(self.vline_operating)])
            {
                let index = <LineIndex as Into<usize>>::into(vline.index);

                if let Some(v_char) = vline.chars.get_mut(index) {
                    v_char.c = character;
                    v_char.frcolor = frcolor;
                    v_char.bkcolor = bkcolor;
                }
                line_index = vline.index;
                vline.index = vline.index + 1;
            }

            self.textui_refresh_characters(self.vline_operating, line_index, 1)?;

            // 加入光标后，因为会识别光标，所以需超过该行最大字符数才能创建新行
            if !line_index.check(self.chars_per_line - 1) {
                self.textui_new_line()?;
            }
        } else {
            // todo: 支持纯文本字符
            todo!();
        }
        return Ok(0);
    }
    /// 根据输入的一个字符在窗口上输出
    /// ## 参数

    /// - window 窗口
    /// - character 字符
    /// - FRcolor 前景色（RGB）
    /// - BKcolor 背景色（RGB）

    fn textui_putchar_window(
        &mut self,
        character: u8,
        frcolor: FontColor,
        bkcolor: FontColor,
    ) -> Result<i32, SystemError> {
        let actual_line_sum = textui_framework().actual_line.load(Ordering::SeqCst);

        //字符'\0'代表ASCII码表中的空字符,表示字符串的结尾
        if unlikely(character == b'\0') {
            return Ok(0);
        }
        // 暂不支持纯文本窗口
        if !self.flags.contains(WindowFlag::TEXTUI_IS_CHROMATIC) {
            return Ok(0);
        }

        //进行换行操作
        if character == b'\n' {
            // 换行时还需要输出\r
            c_uart_send(UartPort::COM1.to_u16(), b'\r');
            self.textui_new_line()?;

            return Ok(0);
        }
        // 输出制表符
        else if character == b'\t' {
            if let TextuiVline::Chromatic(vline) =
                &self.vlines[<LineId as Into<usize>>::into(self.vline_operating)]
            {
                //打印的空格数（注意将每行分成一个个表格，每个表格为8个字符）
                let mut space_to_print = 8 - <LineIndex as Into<usize>>::into(vline.index) % 8;
                while space_to_print > 0 {
                    self.true_textui_putchar_window(b' ', frcolor, bkcolor)?;
                    space_to_print -= 1;
                }
            }
        }
        // 字符 '\x08' 代表 ASCII 码中的退格字符。它在输出中的作用是将光标向左移动一个位置，并在该位置上输出后续的字符，从而实现字符的删除或替换。
        else if character == b'\x08' {
            let mut tmp = LineIndex(0);
            if let TextuiVline::Chromatic(vline) =
                &mut self.vlines[<LineId as Into<usize>>::into(self.vline_operating)]
            {

                vline.index = vline.index - 1;
                tmp = vline.index;
            }
            if <LineIndex as Into<i32>>::into(tmp) >= 0 {
                if let TextuiVline::Chromatic(vline) =
                    &mut self.vlines[<LineId as Into<usize>>::into(self.vline_operating)]
                {
                    if let Some(v_char) = vline.chars.get_mut(<LineIndex as Into<usize>>::into(tmp))
                    {
                        v_char.c = b' ';

                        v_char.bkcolor = bkcolor;
                    }
                }
                return self.textui_refresh_characters(self.vline_operating, tmp, 1);
            }
            // 需要向上缩一行
            if <LineIndex as Into<i32>>::into(tmp) < 0 {
                // 当前行为空,需要重新刷新
                if let TextuiVline::Chromatic(vline) =
                    &mut self.vlines[<LineId as Into<usize>>::into(self.vline_operating)]
                {
                    vline.index = LineIndex::new(0);
                    for i in 0..self.chars_per_line {
                        if let Some(v_char) = vline.chars.get_mut(i as usize) {
                            v_char.c = 0;
                            v_char.frcolor = FontColor::BLACK;
                            v_char.bkcolor = FontColor::BLACK;
                        }
                    }
                }
                // 上缩一行
                self.vline_operating = self.vline_operating - 1;
                if self.vline_operating.data() < 0 {
                    self.vline_operating = LineId(self.vline_sum - 1);
                }

                // 考虑是否向上滚动（在top_vline上退格）
                if self.vlines_used > actual_line_sum {
                    self.top_vline = self.top_vline - 1;
                    if <LineId as Into<i32>>::into(self.top_vline) < 0 {
                        self.top_vline = LineId(self.vline_sum - 1);
                    }
                }
                //因为上缩一行所以显示在屏幕中的虚拟行少一
                self.vlines_used -= 1;
                self.textui_refresh_vlines(self.top_vline, actual_line_sum)?;
            }
        } else {
            // 输出其他字符
            c_uart_send(UartPort::COM1.to_u16(), character);
            if let TextuiVline::Chromatic(vline) =
                &self.vlines[<LineId as Into<usize>>::into(self.vline_operating)]
            {
                if !vline.index.check(self.chars_per_line) {
                    self.textui_new_line()?;
                }

                return self.true_textui_putchar_window(character, frcolor, bkcolor);
            }
        }

        return Ok(0);
    }
}
impl Default for TextuiWindow {
    fn default() -> Self {
        TextuiWindow {
            id: WindowId(0),
            flags: WindowFlag::TEXTUI_IS_CHROMATIC,
            vline_sum: 0,
            vlines_used: 1,
            top_vline: LineId::new(0),
            vlines: Vec::new(),
            vline_operating: LineId::new(0),
            chars_per_line: 0,
        }
    }
}

#[derive(Debug)]
pub struct TextUiFramework {
    metadata: ScmUiFrameworkMetadata,
    // private_info: TextuiPrivateInfo,
    window_list: Arc<SpinLock<LinkedList<Arc<SpinLock<TextuiWindow>>>>>,
    actual_line: AtomicI32, // 真实行的数量（textui的帧缓冲区能容纳的内容的行数）
    current_window: Arc<SpinLock<TextuiWindow>>, // 当前的主窗口
    default_window: Arc<SpinLock<TextuiWindow>>, // 默认print到的窗口
}

impl TextUiFramework {
    pub fn new(
        current_window: Arc<SpinLock<TextuiWindow>>,
        default_window: Arc<SpinLock<TextuiWindow>>,
    ) -> Self {
        let mut inner = TextUiFramework {
            metadata: ScmUiFrameworkMetadata::new("TextUI".to_string(), ScmFramworkType::Text),
            // private_info: TextuiPrivateInfo::new(),
            window_list: Arc::new(SpinLock::new(LinkedList::new())),
            actual_line: AtomicI32::new(0),
            current_window,
            default_window,
        };
        inner.actual_line =
            AtomicI32::new((inner.metadata.buf_info.buf_height() / TEXTUI_CHAR_HEIGHT) as i32);

        return inner;
    }

    /// 将窗口的帧缓冲区内容清零
    pub fn _renew_buf(&self) {
        // let mut addr: *mut u32 = fb as *mut u32;
        let mut addr: *mut u32 = self.metadata.buf_info.vaddr() as *mut u32;

        for _i in 0..self.metadata.buf_info.buf_size_about_u8() {
            unsafe { *addr = 0 };
            unsafe {
                addr = (addr.offset(1)) as *mut u32;
            }
        }
    }
}
// #[derive(Debug)]
// pub struct LockedTextUiFramework(pub SpinLock<TextUiFramework>);
// impl LockedTextUiFramework {
//     pub fn new(
//         actual_line: i32,
//         current_window: Arc<SpinLock<TextuiWindow>>,
//         default_window: Arc<SpinLock<TextuiWindow>>,
//     ) -> Self {
//         let inner = TextUiFramework::new(actual_line, current_window, default_window);
//         let result = Self(SpinLock::new(inner));
//         return result;
//     }
// }

impl ScmUiFramework for &mut TextUiFramework {
    // 安装ui框架的回调函数
    fn install(&self) -> Result<i32, SystemError> {
        c_uart_send_str(
            UartPort::COM1.to_u16(),
            "\ntextui_install_handler\n\0".as_ptr(),
        );
        return Ok(0);
    }
    // 卸载ui框架的回调函数
    fn uninstall(&self) -> Result<i32, SystemError> {
        return Ok(0);
    }
    // 启用ui框架的回调函数
    fn enable(&self) -> Result<i32, SystemError> {
        unsafe { ENABLE_PUT_TO_WINDOW = true };
        return Ok(0);
    }
    // 禁用ui框架的回调函数
    fn disable(&self) -> Result<i32, SystemError> {
        unsafe { ENABLE_PUT_TO_WINDOW = false };
        return Ok(0);
    }
    // 改变ui框架的帧缓冲区的回调函数
    fn change(&self, buf: ScmBufferInfo) -> Result<i32, SystemError> {

        let src = self.metadata.buf_info.vaddr() as *const u8;
        let dst = buf.vaddr() as *mut u8;
        let count = self.metadata.buf_info.buf_size_about_u8() as usize;
        unsafe { copy_nonoverlapping(src, dst, count) };
        textui_framework().metadata.buf_info = buf;

        return Ok(0);
    }
    ///  获取ScmUiFramework的元数据
    ///  ## 返回值
    /// 
    ///  -成功：Ok(ScmUiFramework的元数据)
    ///  -失败：Err(错误码)
    fn metadata(&self) -> Result<ScmUiFrameworkMetadata, SystemError> {
        // let framework_guard = self.0.lock();
        let metadata = self.metadata.clone();
        // drop(framework_guard);
        return Ok(metadata);
    }
}

impl Deref for TextUiFramework {
    type Target = ScmUiFrameworkMetadata;

    fn deref(&self) -> &Self::Target {
        &self.metadata
    }
}

impl DerefMut for TextUiFramework {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.metadata
    }
}

/// 在默认窗口上输出一个字符
/// ## 参数
/// - character 字符
/// - FRcolor 前景色（RGB）
/// - BKcolor 背景色（RGB）

#[no_mangle]
pub extern "C" fn textui_putchar(character: u8, fr_color: u32, bk_color: u32) -> i32 {
    let result;
    if unsafe { TEST_IS_INIT } {
        result = textui_framework()
            .current_window
            .lock()
            .textui_putchar_window(
                character,
                FontColor::from(fr_color),
                FontColor::from(bk_color),
            )
            .unwrap_or_else(|e| e.to_posix_errno());
    } else {
        //未初始化暴力输出
        result = no_init_textui_putchar_window(
            character,
            FontColor::from(fr_color),
            FontColor::from(bk_color),
            unsafe { ENABLE_PUT_TO_WINDOW },
        )
        .unwrap_or_else(|e| e.to_posix_errno());
    }
    if result.is_negative() {
        c_uart_send_str(
            UartPort::COM1.to_u16(),
            "textui putchar failed.\n\0".as_ptr(),
        );
    }
    return result;
}


/// 初始化text ui框架

#[no_mangle]
pub extern "C" fn rs_textui_init() -> i32 {
    let r = textui_init().unwrap_or_else(|e| e.to_posix_errno());
    if r.is_negative() {
        c_uart_send_str(UartPort::COM1.to_u16(), "textui init failed.\n\0".as_ptr());
    }
    return r;
}

fn textui_init() -> Result<i32, SystemError> {
    unsafe { textui_framwork_init() };

    let textui_framework = textui_framework();

    // 为textui框架生成第一个窗口
    let vlines_num =
        (textui_framework.metadata.buf_info.buf_height() / TEXTUI_CHAR_HEIGHT) as usize;

    let chars_num = (textui_framework.metadata.buf_info.buf_width() / TEXTUI_CHAR_WIDTH) as usize;

    let initial_window = TextuiWindow::new(
        WindowFlag::TEXTUI_IS_CHROMATIC,
        vlines_num as i32,
        chars_num as i32,
    );

    textui_framework.current_window = Arc::new(SpinLock::new(initial_window));

    textui_framework.default_window = textui_framework.current_window.clone();

    // 添加进textui框架的窗口链表中
    textui_framework
        .window_list
        .lock()
        .push_back(textui_framework.current_window.clone());

    unsafe { TEST_IS_INIT = true };

    scm_register(Arc::new(textui_framework))?;
    
    c_uart_send_str(
        UartPort::COM1.to_u16(),
        "\ntext ui initialized\n\0".as_ptr(),
    );
    return Ok(0);
}
