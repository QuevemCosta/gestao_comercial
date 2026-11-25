//! Camada de apresentação (Slint).
//!
//! `format` converte valores digitados e exibidos, `controller` traduz ações da tela em
//! casos de uso e `app` liga os componentes da janela aos controladores.
//! Nenhuma regra de negócio ou SQL vive nesta camada.

pub mod app;
pub mod controller;
pub mod format;

slint::include_modules!();

pub use app::run;