use tauri::{AppHandle, Manager};

enum CalculationStatus {
    Success,
    Error,
}

struct MyStruct {
    field: String,
}

fn emit_scoped_identifier(app: &AppHandle) {
    app.emit("calc-status", CalculationStatus::Success).unwrap();
}

fn emit_struct_expression(app: &AppHandle) {
    app.emit("struct-event", MyStruct { field: "hello".to_string() }).unwrap();
}

fn emit_string_literal(app: &AppHandle) {
    app.emit("string-event", "hello world").unwrap();
}

fn emit_typed_variable(app: &AppHandle) {
    let status: CalculationStatus = CalculationStatus::Success;
    app.emit("typed-var-event", status).unwrap();
}

fn emit_inferred_variable(app: &AppHandle) {
    let status = CalculationStatus::Error;
    app.emit("inferred-var-event", status).unwrap();
}
