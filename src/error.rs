use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("配置错误: {0}")]
    Config(String),

    #[error("网络错误: {0}")]
    Network(String),

    #[error("校验失败: {0}")]
    Verify(String),

    #[error("安装失败: {0}")]
    Install(String),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// 目标文件被占用（旧进程还没退出）属于可重试错误。
    pub fn is_transient(&self) -> bool {
        match self {
            AppError::Io(error) => matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::WouldBlock
            ),
            _ => false,
        }
    }
}
