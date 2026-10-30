#include <dlfcn.h>
#include <stdio.h>
#include <string.h>

static int invoke(void *provider, int expect_hook) {
    void (*swap)(void *, unsigned long) = dlsym(provider, "glXSwapBuffers");
    void (*destroy)(void *, void *) = dlsym(provider, "glXDestroyContext");
    int (*counts)(void) = dlsym(provider, "fixture_counts");
    void *swap_hook = dlsym(RTLD_DEFAULT, "glXSwapBuffers");
    void *destroy_hook = dlsym(RTLD_DEFAULT, "glXDestroyContext");
    if (!swap || !destroy || !counts || dlerror()) return 2;
    if (((void *)swap == swap_hook) != expect_hook ||
        ((void *)destroy == destroy_hook) != expect_hook) return 3;
    swap(NULL, 0); destroy(NULL, NULL);
    if (counts() != 11) return 4;
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 4) return 5;
    const char *mode = argv[1];
    int global = strcmp(mode, "global") == 0;
    void *first = NULL;
    if (strcmp(mode, "other-provider") == 0) {
        first = dlopen(argv[3], RTLD_NOW | RTLD_GLOBAL);
        if (!first || invoke(first, 1)) return 6;
    }
    if (strcmp(mode, "cached-miss") == 0) {
        void (*swap)(void *, unsigned long) = dlsym(RTLD_DEFAULT, "glXSwapBuffers");
        void (*destroy)(void *, void *) = dlsym(RTLD_DEFAULT, "glXDestroyContext");
        if (!swap || !destroy) return 7;
        swap(NULL, 0); destroy(NULL, NULL);
        global = 1;
    }
    void *provider = dlopen(argv[2], RTLD_NOW | (global ? RTLD_GLOBAL : RTLD_LOCAL));
    if (!provider) return 8;
    if (strcmp(mode, "promoted") == 0) {
        // A local successful lookup must not cache a failed RTLD_NEXT probe.
        if (!dlsym(provider, "glXSwapBuffers") || dlerror() ||
            !dlsym(provider, "glXDestroyContext") || dlerror()) return 9;
        if (!dlopen(argv[2], RTLD_NOW | RTLD_NOLOAD | RTLD_GLOBAL)) return 10;
        global = 1;
    }
    if (dlsym(provider, "fixture_missing_symbol") || !dlerror()) return 11;
    int result = invoke(provider, global && strcmp(mode, "cached-miss") != 0);
    if (first) {
        int (*counts)(void) = dlsym(first, "fixture_counts");
        if (!counts || counts() != 11) return 12;
    }
    printf("%s: result=%d\n", mode, result);
    dlclose(provider);
    if (first) dlclose(first);
    return result;
}
