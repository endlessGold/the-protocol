(module
  ;; Linear memory (64KB initial, no max)
  (memory (export "memory") 1)

  ;; Heap pointer for bump allocator
  (global $heap_ptr (mut i32) (i32.const 65536))

  ;; allocate_buffer(size: i32) -> i32 (pointer)
  ;; Bump allocator: returns current heap pointer, advances by size (aligned to 8 bytes)
  (func (export "allocate_buffer") (param $size i32) (result i32)
    (local $ptr i32)
    (local $aligned_size i32)
    (local.set $ptr (global.get $heap_ptr))
    ;; Align to 8 bytes
    (local.set $aligned_size
      (i32.add
        (local.get $size)
        (i32.and
          (i32.sub (i32.const 8) (i32.rem_u (local.get $size) (i32.const 8)))
          (i32.const 7)
        )
      )
    )
    (global.set $heap_ptr
      (i32.add (global.get $heap_ptr) (local.get $aligned_size))
    )
    (local.get $ptr)
  )

  ;; free_buffer(ptr: i32, size: i32) -> void
  ;; No-op for bump allocator
  (func (export "free_buffer") (param $ptr i32) (param $size i32)
    nop
  )

  ;; ===== Plugin Lifecycle =====

  (func (export "plugin_init") (result i32)
    ;; Log: level=2 (INFO), msg="Hello World plugin initialized!"
    ;; We'll just return 0 (success) for now
    i32.const 0
  )

  (func (export "plugin_enable") (result i32)
    i32.const 0
  )

  (func (export "plugin_disable") (result i32)
    i32.const 0
  )

  (func (export "plugin_unload") (result i32)
    i32.const 0
  )

  ;; ===== Command Handler =====

  ;; handle_command(cmd_ptr, cmd_len, args_ptr, args_len, player_id) -> i32
  (func (export "handle_command")
    (param $cmd_ptr i32) (param $cmd_len i32)
    (param $args_ptr i32) (param $args_len i32)
    (param $player_id i64)
    (result i32)
    ;; Return 0 (success)
    i32.const 0
  )

  ;; ===== Event Handler =====

  ;; handle_event(type_ptr, type_len, data_ptr, data_len) -> i32
  (func (export "handle_event")
    (param $type_ptr i32) (param $type_len i32)
    (param $data_ptr i32) (param $data_len i32)
    (result i32)
    ;; Return 0 (success)
    i32.const 0
  )
)
