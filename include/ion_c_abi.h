#ifndef ION_C_ABI_H
#define ION_C_ABI_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * IONVM Plugin Magic identifier: "IONV" (0x494F4E56 in little-endian).
 * Every IonVM native plugin library must export:
 *     uint32_t IONVM_PLUGIN_MAGIC = 0x494F4E56;
 */
#define IONVM_PLUGIN_MAGIC_VAL 0x494F4E56

/* IonCValue Type Tags */
#define ION_VAL_NIL     0
#define ION_VAL_INT     1
#define ION_VAL_FLOAT   2
#define ION_VAL_BOOL    3
#define ION_VAL_STRING  4
#define ION_VAL_POINTER 5

/* IonVM Return Type Tags */
#define ION_TYPE_ANY     0
#define ION_TYPE_VOID    1
#define ION_TYPE_INT     2
#define ION_TYPE_FLOAT   3
#define ION_TYPE_BOOLEAN 4
#define ION_TYPE_STRING  5
#define ION_TYPE_TUPLE   6
#define ION_TYPE_ARRAY   7
#define ION_TYPE_OBJECT  8

/**
 * Tagged union representing values passed between IonVM and native C/C++/Fortran/Pascal functions.
 * Memory layout is 16 bytes:
 *   [0..3]  int32_t tag
 *   [4..7]  int32_t _reserved (alignment padding)
 *   [8..15] union data (64-bit integer, 64-bit double, bool, pointer, string)
 */
typedef struct IonCValue {
    int32_t tag;
    int32_t _reserved;
    union {
        int64_t int_val;
        double float_val;
        int32_t bool_val;
        const char* string_val;
        void* ptr_val;
    } data;
} IonCValue;

/**
 * Standard C function signature for native functions called from IonVM.
 *
 * @param args Pointer to an array of IonCValue arguments.
 * @param argc Number of arguments passed.
 * @param out  Pointer to an IonCValue buffer provided by IonVM for the return value.
 * @return 0 on success, non-zero error code on failure.
 *
 * Fortran mapping:
 *   function my_func(args, argc, out_val) bind(C, name="my_func")
 *     use iso_c_binding
 *     type(c_ptr), value :: args
 *     integer(c_size_t), value :: argc
 *     type(c_ptr), value :: out_val
 *     integer(c_int32_t) :: my_func
 *
 * Pascal mapping:
 *   function MyFunc(args: PIonCValue; argc: csize_t; out_val: PIonCValue): cint32; cdecl;
 */
typedef int32_t (*IonCFunction)(const IonCValue* args, size_t argc, IonCValue* out);

/**
 * Registration callback passed by IonVM to ionvm_plugin_init.
 *
 * @param reg_ctx     Opaque registry pointer provided by IonVM.
 * @param name        Null-terminated function name (e.g., "gui.show_message" or "math.custom_sin").
 * @param func        Function pointer conforming to IonCFunction.
 * @param arity       Expected number of arguments.
 * @param return_type Expected return type tag (e.g., ION_TYPE_INT).
 * @return 0 on success, non-zero on failure.
 */
typedef int32_t (*IonCRegisterCallback)(
    void* reg_ctx,
    const char* name,
    IonCFunction func,
    size_t arity,
    uint8_t return_type
);

/**
 * Entry point that must be exported by C-ABI plugins.
 *
 * @param reg_ctx  Opaque registry pointer passed by IonVM.
 * @param reg_cb   Registration callback to register functions into IonVM.
 * @return 0 on success, non-zero on error.
 */
int32_t ionvm_plugin_init(void* reg_ctx, IonCRegisterCallback reg_cb);

/**
 * Yield execution from a native C function back to IonVM to execute a callback function.
 *
 * Execution in C pauses immediately at this call. IonVM runs the specified callback
 * with `args`, writes the return value into `*out`, and resumes the C function on the
 * next line.
 *
 * @param callback_name Null-terminated name of the IonVM / builtin function to call.
 * @param args          Arguments passed to the callback.
 * @param argc          Number of arguments passed.
 * @param out           Buffer where IonVM writes the callback's return value.
 * @return 0 on success, non-zero on failure.
 */
typedef int32_t (*IonCYieldFn)(const char* callback_name, const IonCValue* args, size_t argc, IonCValue* out);

extern IonCYieldFn ion_c_yield;
void ion_set_yield_fn(IonCYieldFn fn);

#ifdef __cplusplus
}
#endif

#endif /* ION_C_ABI_H */
