//! 由 SyPetype PCPN Simulator 自动生成
//! 
//! 此代码展示了一条通过 Rust 借用检查的 API 调用序列。
//! 所有借用都是显式的，使用 drop() 显式结束借用。

#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(dead_code)]

fn main() {
    let v0 = Stack<Stack::T>::new(); // Stack<Stack::T>::new
    let v1 = Stack<Stack::T>::new(); // Stack<Stack::T>::new
    let v2 = Stack<Stack::T>::new(); // Stack<Stack::T>::new
    let v3 = Counter::new(); // Counter::new
    let v4 = Counter::new(); // Counter::new
}