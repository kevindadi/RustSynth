//! 由 SyPetype PCPN Simulator 自动生成
//! 
//! 此代码展示了一条通过 Rust 借用检查的 API 调用序列。
//! 所有借用都是显式的，使用 drop() 显式结束借用。

#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(dead_code)]

fn main() {
    let v0 = Counter::new(); // Counter::new
    let v1 = Counter::new(); // Counter::new
    let v2 = Counter::new(); // Counter::new
    let mut v0 = v0; // let mut = Counter
    let mut v1 = v1; // let mut = Counter
}