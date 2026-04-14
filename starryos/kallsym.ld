SECTIONS {
    __static_keys : ALIGN(0x10) {
        __start___static_keys = .;
        KEEP(*(__static_keys))
        __stop___static_keys = .;
    }
    
    .tracepoint : ALIGN(0x10) {
        __start_tracepoint = .;
        KEEP(*(.tracepoint .tracepoint.*))
        __stop_tracepoint = .;
    }

    .kallsyms : ALIGN(4K) {
        __kallsyms_start = .;
        . += 8M; /* reserve space for kallsyms, can be recycled */
        __kallsyms_end = .;
    }

}

INSERT AFTER .data;