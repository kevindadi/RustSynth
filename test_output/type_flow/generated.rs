//! 由 SyPetype PCPN Simulator 自动生成
//! 
//! 此代码展示了一条通过 Rust 借用检查的 API 调用序列。
//! 所有借用都是显式的，使用 drop() 显式结束借用。

#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(dead_code)]

fn main() {
    let mut v0 = Sink::new(); // Sink::new
    let mut v1 = Sink::new(); // Sink::new
    let r2 = &v0; // &Sink [first]
    let mut v3 = Sink::new(); // Sink::new
    drop(v1); // drop Sink
    drop(r2); // drop &Sink [unfreeze]
}