#include <gtk/gtk.h>
#include <stdio.h>
#include <stdlib.h>
#include "include/ion_c_abi.h"

extern "C" {

uint32_t IONVM_PLUGIN_MAGIC = 0x494F4E56;

IonCYieldFn ion_c_yield = nullptr;

void ion_set_yield_fn(IonCYieldFn fn) {
    ion_c_yield = fn;
}

static gboolean close_dialog_timeout(gpointer data) {
    GtkDialog *dialog = GTK_DIALOG(data);
    gtk_dialog_response(dialog, GTK_RESPONSE_OK);
    return FALSE;
}

int32_t gui_show_message(const IonCValue* args, size_t argc, IonCValue* out) {
    const char* message = (argc > 0 && args[0].tag == ION_VAL_STRING && args[0].data.string_val)
                          ? args[0].data.string_val
                          : "Message from IonVM Plugin";

    const char* title = (argc > 1 && args[1].tag == ION_VAL_STRING && args[1].data.string_val)
                        ? args[1].data.string_val
                        : "IonVM Dialog";

    int response = 1;

    if (gtk_init_check(nullptr, nullptr)) {
        GtkWidget *dialog = gtk_message_dialog_new(
            nullptr,
            GTK_DIALOG_MODAL,
            GTK_MESSAGE_INFO,
            GTK_BUTTONS_OK,
            "%s", message
        );
        gtk_window_set_title(GTK_WINDOW(dialog), title);

        // Auto-close dialog with a fast timeout (150ms default or IONVM_GTK_TIMEOUT_MS) so tests don't require clicking OK
        guint timeout_ms = 150;
        const char* env_timeout = getenv("IONVM_GTK_TIMEOUT_MS");
        if (env_timeout) {
            timeout_ms = (guint)atoi(env_timeout);
        }
        g_timeout_add(timeout_ms, close_dialog_timeout, dialog);

        response = gtk_dialog_run(GTK_DIALOG(dialog));
        gtk_widget_destroy(dialog);

        while (gtk_events_pending()) {
            gtk_main_iteration();
        }
    } else {
        printf("[libgui headless fallback] %s: %s\n", title, message);
        response = 1;
    }

    out->tag = ION_VAL_INT;
    out->_reserved = 0;
    out->data.int_val = (int64_t)response;
    return 0;
}

int32_t gui_yield_calc(const IonCValue* args, size_t argc, IonCValue* out) {
    int64_t base = (argc > 0 && args[0].tag == ION_VAL_INT) ? args[0].data.int_val : 50;

    // Yield to IonVM to calculate sqrt(100.0)
    IonCValue yield_arg;
    yield_arg.tag = ION_VAL_FLOAT;
    yield_arg.data.float_val = 100.0;

    IonCValue vm_res;
    vm_res.tag = ION_VAL_NIL;

    // Pauses this C function and switches to IonVM to execute "sqrt"
    int32_t status = ion_c_yield("sqrt", &yield_arg, 1, &vm_res);
    if (status != 0) {
        return status;
    }

    double sqrt_val = (vm_res.tag == ION_VAL_FLOAT) ? vm_res.data.float_val : 0.0;

    // Resumes here with the result from IonVM!
    out->tag = ION_VAL_INT;
    out->_reserved = 0;
    out->data.int_val = base + (int64_t)sqrt_val; // base (50) + sqrt(100) = 60
    return 0;
}

int32_t ionvm_plugin_init(void* reg_ctx, IonCRegisterCallback register_fn) {
    register_fn(reg_ctx, "gui.show_message", gui_show_message, 2, ION_TYPE_INT);
    return register_fn(reg_ctx, "gui.yield_calc", gui_yield_calc, 1, ION_TYPE_INT);
}

int32_t run_register(void* reg_ctx, IonCRegisterCallback register_fn) {
    return ionvm_plugin_init(reg_ctx, register_fn);
}

}
