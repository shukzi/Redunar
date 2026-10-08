#include <stddef.h>

static int swaps;
static int destroys;
void glXSwapBuffers(void *display, unsigned long drawable) {
    (void)display; (void)drawable; ++swaps;
}
void glXDestroyContext(void *display, void *context) {
    (void)display; (void)context; ++destroys;
}
void *glXGetCurrentContext(void) { return NULL; }
int fixture_counts(void) { return swaps * 10 + destroys; }
