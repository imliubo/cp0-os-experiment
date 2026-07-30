#include "seccomp.h"

#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include "wasm_export.h"

#define CP0_MAX_MODULE_BYTES (16U * 1024U * 1024U)
#define CP0_WASM_STACK_BYTES (64U * 1024U)
#define CP0_WASM_HEAP_BYTES (64U * 1024U)
#define CP0_ERROR_BYTES 256U

static bool valid_module_path(const char *path) {
    static const char prefix[] = "/app/";
    size_t length;

    if (path == NULL)
        return false;
    length = strlen(path);
    return length > sizeof(prefix) - 1U &&
           strncmp(path, prefix, sizeof(prefix) - 1U) == 0 &&
           strstr(path, "/../") == NULL && strstr(path, "//") == NULL &&
           (length < 3U || strcmp(path + length - 3U, "/..") != 0);
}

static uint8_t *read_module(const char *path, uint32_t *size_out) {
    struct stat metadata;
    uint8_t *buffer = NULL;
    size_t offset = 0;
    int descriptor = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (descriptor < 0) {
        fprintf(stderr, "app-runtime: cannot open module: %s\n", strerror(errno));
        return NULL;
    }
    if (fstat(descriptor, &metadata) != 0 || !S_ISREG(metadata.st_mode) ||
        metadata.st_size <= 0 ||
        (uint64_t)metadata.st_size > CP0_MAX_MODULE_BYTES) {
        fprintf(stderr, "app-runtime: module is not a valid regular file\n");
        close(descriptor);
        return NULL;
    }

    *size_out = (uint32_t)metadata.st_size;
    buffer = malloc(*size_out);
    if (buffer == NULL) {
        fprintf(stderr, "app-runtime: cannot allocate module buffer\n");
        close(descriptor);
        return NULL;
    }
    while (offset < *size_out) {
        ssize_t count = read(descriptor, buffer + offset, *size_out - offset);
        if (count <= 0) {
            fprintf(stderr, "app-runtime: cannot read complete module\n");
            free(buffer);
            close(descriptor);
            return NULL;
        }
        offset += (size_t)count;
    }
    close(descriptor);
    return buffer;
}

int main(int argc, char **argv) {
    RuntimeInitArgs init_args;
    wasm_module_t module = NULL;
    wasm_module_inst_t instance = NULL;
    uint8_t *module_bytes = NULL;
    uint32_t module_size = 0;
    char error[CP0_ERROR_BYTES] = {0};
    int result = EXIT_FAILURE;

    if (argc != 2 || !valid_module_path(argv[1])) {
        fprintf(stderr,
                "usage: cardputerzero-app-runtime /app/<module.wasm|module.aot>\n");
        return 2;
    }
    module_bytes = read_module(argv[1], &module_size);
    if (module_bytes == NULL)
        return EXIT_FAILURE;

    memset(&init_args, 0, sizeof(init_args));
    init_args.mem_alloc_type = Alloc_With_System_Allocator;
    if (!wasm_runtime_full_init(&init_args)) {
        fprintf(stderr, "app-runtime: WAMR initialization failed\n");
        goto cleanup;
    }
    if (cp0_install_runtime_seccomp() != 0) {
        fprintf(stderr, "app-runtime: seccomp setup failed: %s\n", strerror(errno));
        goto runtime_cleanup;
    }

    module = wasm_runtime_load(module_bytes, module_size, error, sizeof(error));
    if (module == NULL) {
        fprintf(stderr, "app-runtime: module load failed: %s\n", error);
        goto runtime_cleanup;
    }
    instance = wasm_runtime_instantiate(module, CP0_WASM_STACK_BYTES,
                                        CP0_WASM_HEAP_BYTES, error,
                                        sizeof(error));
    if (instance == NULL) {
        fprintf(stderr, "app-runtime: module instantiate failed: %s\n", error);
        goto module_cleanup;
    }
    if (!wasm_application_execute_main(instance, 0, NULL)) {
        const char *exception = wasm_runtime_get_exception(instance);
        fprintf(stderr, "app-runtime: application failed: %s\n",
                exception != NULL ? exception : "unknown exception");
        goto instance_cleanup;
    }
    result = EXIT_SUCCESS;

instance_cleanup:
    wasm_runtime_deinstantiate(instance);
module_cleanup:
    wasm_runtime_unload(module);
runtime_cleanup:
    wasm_runtime_destroy();
cleanup:
    free(module_bytes);
    return result;
}
