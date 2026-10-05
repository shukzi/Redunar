/* No Vulkan device or window. Export competing loader symbols before dlopen to
 * reproduce ELF preemption in a game linked directly against libvulkan. */
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef void (*Function)(void);
typedef Function (*GetProc)(void *, const char *);
typedef struct {
    int type;
    void *next;
    uint32_t version;
    GetProc instance, device, physical;
} Negotiation;
typedef int (*Negotiate)(Negotiation *);

Function vkGetInstanceProcAddr(void *instance, const char *name) {
    (void)instance; (void)name; return NULL;
}
Function vkGetDeviceProcAddr(void *device, const char *name) {
    (void)device; (void)name; return NULL;
}
void vkCreateInstance(void) {}
void vkCreateDevice(void) {}
void vkQueuePresentKHR(void) {}

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    void *layer = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!layer) { fprintf(stderr, "%s\n", dlerror()); return 1; }
    void *raw = dlsym(layer, "vkNegotiateLoaderLayerInterfaceVersion");
    Negotiate negotiate = NULL;
    memcpy(&negotiate, &raw, sizeof(negotiate));
    Negotiation interface = {.type = 1, .version = 2};
    int failed = !negotiate || negotiate(&interface) != 0;
    const char *names[] = {"vkGetInstanceProcAddr", "vkGetDeviceProcAddr",
        "vkCreateInstance", "vkCreateDevice", "vkQueuePresentKHR"};
    for (unsigned i = 0; !failed && i < sizeof(names) / sizeof(names[0]); ++i) {
        Function expected = NULL;
        raw = dlsym(layer, names[i]);
        memcpy(&expected, &raw, sizeof(expected));
        GetProc get = i == 4 ? interface.device : interface.instance;
        Function actual = get ? get(NULL, names[i]) : NULL;
        if (!expected || actual != expected) {
            fprintf(stderr, "Vulkan layer hook was preempted: %s\n", names[i]); failed = 1;
        }
    }
    dlclose(layer);
    if (!failed) puts("Vulkan layer hooks retain their own addresses with competing loader exports.");
    return failed;
}
