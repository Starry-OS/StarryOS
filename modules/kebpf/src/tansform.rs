use axerrno::{AxError, AxResult};
use kbpf_basic::BpfError;




pub fn bpferror_to_axresult(err: BpfError) -> AxResult<isize> {
    Err(bpferror_to_axerr(err))
}

pub fn bpferror_to_axerr(err: BpfError) -> AxError {
    match err {
        BpfError::InvalidArgument => AxError::InvalidInput,
        BpfError::NotFound => AxError::NotFound,
        BpfError::NotSupported => AxError::OperationNotSupported,
        BpfError::NoSpace => AxError::NoMemory,
        BpfError::TooBig => AxError::Other(axerrno::LinuxError::E2BIG),
        BpfError::TryAgain => AxError::Other(axerrno::LinuxError::EAGAIN),
    }
}