#define _POSIX_C_SOURCE 200809L
#include "../../crates/redunar-capture-opengl/src/replay_export/nvidia_memory.c"
#include <assert.h>
#include <stdio.h>
static VkResult submit_result, fence_result, reset_result;
static unsigned destroyed, submitted;
static VkResult VKAPI_CALL fake_submit(VkQueue q,uint32_t n,const VkSubmitInfo *s,VkFence f) {
    (void)q;(void)f;assert(n==1);assert(s->waitSemaphoreCount==1);
    assert(s->signalSemaphoreCount==1);assert(*s->pWaitDstStageMask==VK_PIPELINE_STAGE_TRANSFER_BIT);
    submitted++;return submit_result;
}
static VkResult VKAPI_CALL fake_status(VkDevice d,VkFence f) {(void)d;(void)f;return fence_result;}
static VkResult VKAPI_CALL fake_reset(VkDevice d,uint32_t n,const VkFence *f) {(void)d;(void)n;(void)f;return reset_result;}
static void VKAPI_CALL fake_destroy(VkDevice d,VkFence f,const VkAllocationCallbacks *a) {(void)d;(void)f;(void)a;destroyed++;}
static Allocation *owner(void) {
    Allocation *p=calloc(1,sizeof(*p));assert(p);p->state=GL_OWNED;p->fence=(VkFence)(uintptr_t)1;
    context.owners++;return p;
}
static void setup(void) {
    context.submit=fake_submit;context.fence_status=fake_status;context.reset_fences=fake_reset;context.destroy_fence=fake_destroy;
    submit_result=VK_SUCCESS;fence_result=VK_NOT_READY;reset_result=VK_SUCCESS;
    /* Keep one context reference so tests do not run actual driver teardown. */
    context.owners=1;
}
int main(void) {
    setup();
    Allocation *bad=owner(),*healthy=owner();
    submit_result=VK_ERROR_OUT_OF_DEVICE_MEMORY;
    assert(redunar_nvidia_memory_poll(bad)==-1);
    assert(bad->completion_unproven);
    redunar_nvidia_memory_free(bad);
    assert(retired==bad && destroyed==0);
    submit_result=VK_SUCCESS;
    assert(redunar_nvidia_memory_poll(healthy)==0);
    assert(healthy->state==COPY_PENDING);
    fence_result=VK_ERROR_DEVICE_LOST;
    assert(redunar_nvidia_memory_poll(healthy)==-1);
    redunar_nvidia_memory_free(healthy);
    assert(context.owners==3 && destroyed==0);
    /* A later real completion proof, never an error, permits reclamation. */
    fence_result=VK_SUCCESS;collect_retired();
    assert(!retired && context.owners==1 && destroyed==2);
    Allocation *fresh=owner();
    assert(redunar_nvidia_memory_poll(fresh)==1);
    assert(redunar_nvidia_memory_rearm(fresh)==1);
    assert(fresh->state==GL_OWNED);
    reset_result=VK_ERROR_OUT_OF_HOST_MEMORY;fresh->state=READY;
    assert(redunar_nvidia_memory_rearm(fresh)==-1);
    redunar_nvidia_memory_free(fresh);
    /* Resize/context recreation may destroy a still-initializing owner. */
    Allocation *initial=owner();initial->state=INITIALIZING;initial->completion_unproven=1;
    fence_result=VK_NOT_READY;assert(redunar_nvidia_memory_rearm(initial)==0);
    redunar_nvidia_memory_free(initial);assert(retired==initial);
    fence_result=VK_SUCCESS;collect_retired();assert(!retired && context.owners==1);
    /* Unproven failed submissions remain retained across repeated allocations. */
    Allocation *lost=owner();lost->state=FAILED;lost->completion_unproven=1;
    fence_result=VK_ERROR_DEVICE_LOST;redunar_nvidia_memory_free(lost);
    for(unsigned i=0;i<128;i++)collect_retired();
    assert(retired==lost && context.owners==2);
    fence_result=VK_SUCCESS;collect_retired();assert(!retired && context.owners==1);
    printf("NVIDIA bridge state/lifetime regressions passed; submissions=%u, reclaimed=%u\n",submitted,destroyed);
    return 0;
}
