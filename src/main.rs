pub mod cpn;
pub mod ir_graph;
pub mod parse;

use log::info;
use std::env;
use std::fs::File;
use std::process;
use std::sync::Mutex;

struct FileLogger {
    file: Mutex<File>,
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let msg = format!("[{}] {}", record.level(), record.args());
            println!("{}", msg); // Print to stdout
            use std::io::Write;
            if let Ok(mut file) = self.file.lock() {
                let _ = writeln!(file, "{}", msg); // Write to file
            }
        }
    }

    fn flush(&self) {}
}

fn init_logger() {
    let file = File::create("ir_graph.log").expect("无法创建日志文件");
    let logger = Box::new(FileLogger {
        file: Mutex::new(file),
    });

    // 如果已经初始化过，忽略错误
    let _ = log::set_boxed_logger(logger);
    log::set_max_level(log::LevelFilter::Info);
}

fn main() {
    init_logger();
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("用法: {} <rustdoc-json-file>", args[0]);
        eprintln!("示例: {} ./base64.json", args[0]);
        process::exit(1);
    }

    let json_path = &args[1];

    println!("正在加载 rustdoc JSON: {}", json_path);
    info!("开始解析 rustdoc JSON: {}", json_path);

    // 步骤 1: 解析 rustdoc JSON
    let parsed_crate = match parse::ParsedCrate::from_json_file(json_path) {
        Ok(crate_data) => {
            println!("✓ 成功解析 rustdoc JSON");
            crate_data
        }
        Err(e) => {
            eprintln!("✗ 解析失败: {}", e);
            process::exit(1);
        }
    };

    // 打印解析统计
    parsed_crate.print_stats();
    println!();

    // 步骤 2:  构建 IR 图（中间表示）
    println!("正在构建 IR Graph（中间表示）...");
    let ir_graph = ir_graph::build_ir_graph(parsed_crate);
    println!("✓ IR Graph 构建完成");
    println!();

    // 打印 IR 图统计
    ir_graph.print_stats();
    println!();

    // 步骤 3: 导出图
    println!("=== 导出选项 ===");

    // 导出 IR Graph 为 JSON
    let ir_json_output = ir_graph.export_to_json();
    let ir_json_file = "ir_graph.json";
    if let Err(e) = std::fs::write(
        ir_json_file,
        serde_json::to_string_pretty(&ir_json_output).unwrap(),
    ) {
        eprintln!("✗ 写入 IR JSON 失败: {}", e);
    } else {
        println!("✓ IR Graph JSON 已导出到: {}", ir_json_file);
    }

    // 导出 IR Graph 为 DOT（Petri Net 风格）
    let ir_dot_output = ir_graph.export_to_dot();
    let ir_dot_file = "ir_graph.dot";
    if let Err(e) = std::fs::write(ir_dot_file, ir_dot_output) {
        eprintln!("✗ 写入 IR DOT 失败: {}", e);
    } else {
        println!("✓ IR Graph DOT 已导出到: {}", ir_dot_file);
        println!("  可使用以下命令生成可视化图像:");
        println!("  dot -Tpng {} -o ir_graph.png", ir_dot_file);
    }

    println!("\n✓ 所有步骤完成!");
}
