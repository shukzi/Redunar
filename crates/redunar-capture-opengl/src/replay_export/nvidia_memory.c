/* NVIDIA OpenGL/Vulkan buffer interoperability. No pixels cross this ABI. */
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
#include <dlfcn.h>
#include <pthread.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

typedef struct {
    void *loader;
    VkInstance instance;
    VkPhysicalDevice physical;
    VkDevice device;
    uint8_t uuid[16], driver[16];
    unsigned owners;
    VkQueue queue;
    VkCommandPool pool;
    uint32_t queue_family;
    PFN_vkGetDeviceQueue get_queue;
    PFN_vkCreateCommandPool create_pool;
    PFN_vkDestroyCommandPool destroy_pool;
    PFN_vkAllocateCommandBuffers allocate_commands;
    PFN_vkFreeCommandBuffers free_commands;
    PFN_vkBeginCommandBuffer begin;
    PFN_vkEndCommandBuffer end;
    PFN_vkCmdCopyBuffer copy;
    PFN_vkCmdPipelineBarrier barrier;
    PFN_vkCreateFence create_fence;
    PFN_vkDestroyFence destroy_fence;
    PFN_vkResetFences reset_fences;
    PFN_vkGetFenceStatus fence_status;
    PFN_vkQueueSubmit submit;
    PFN_vkCreateSemaphore create_semaphore;
    PFN_vkDestroySemaphore destroy_semaphore;
    PFN_vkGetSemaphoreFdKHR semaphore_fd;
    PFN_vkDestroyInstance destroy_instance;
    PFN_vkDestroyDevice destroy_device;
    PFN_vkCreateBuffer create_buffer;
    PFN_vkDestroyBuffer destroy_buffer;
    PFN_vkGetBufferMemoryRequirements2 requirements;
    PFN_vkAllocateMemory allocate;
    PFN_vkFreeMemory free_memory;
    PFN_vkBindBufferMemory bind;
    PFN_vkGetMemoryFdKHR get_fd;
    PFN_vkGetPhysicalDeviceMemoryProperties memory_properties;
} Context;
typedef struct Allocation {
    VkBuffer buffer, dma_buffer;
    VkDeviceMemory memory, dma_memory;
    VkCommandBuffer command, initial_command;
    VkFence fence;
    VkSemaphore gl_ready, vk_ready;
    enum { INITIALIZING, GL_OWNED, COPY_PENDING, READY, FAILED } state;
    /* No submit error is a completion proof. A successful fence clears this. */
    int completion_unproven;
    struct Allocation *next;
} Allocation;
static Allocation *retired;
static Context context;
static pthread_mutex_t mutex=PTHREAD_MUTEX_INITIALIZER;

static void close_context(void) {
    if(context.pool && context.destroy_pool)context.destroy_pool(context.device,context.pool,NULL);
    if(context.device && context.destroy_device)context.destroy_device(context.device,NULL);
    if(context.instance && context.destroy_instance)context.destroy_instance(context.instance,NULL);
    if(context.loader)dlclose(context.loader);
    memset(&context,0,sizeof(context));
}

/* Caller holds mutex. Identity is matched before any logical device exists. */
static int open_context(const uint8_t *uuid,const uint8_t *driver) {
    if(context.device)return memcmp(context.uuid,uuid,16)==0 && memcmp(context.driver,driver,16)==0 ? 1 : -1;
    context.loader=dlopen("libvulkan.so.1",RTLD_NOW|RTLD_LOCAL);
    if(!context.loader)return -1;
    PFN_vkGetInstanceProcAddr get=(PFN_vkGetInstanceProcAddr)dlsym(context.loader,"vkGetInstanceProcAddr");
    if(!get)goto fail;
    PFN_vkCreateInstance create=(PFN_vkCreateInstance)get(NULL,"vkCreateInstance");
    if(!create)goto fail;
    VkApplicationInfo app={.sType=VK_STRUCTURE_TYPE_APPLICATION_INFO,.pApplicationName="Redunar OpenGL export",.apiVersion=VK_API_VERSION_1_1};
    VkInstanceCreateInfo ci={.sType=VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,.pApplicationInfo=&app};
    if(create(&ci,NULL,&context.instance)!=VK_SUCCESS)goto fail;
    context.destroy_instance=(PFN_vkDestroyInstance)get(context.instance,"vkDestroyInstance");
    PFN_vkEnumeratePhysicalDevices enumerate=(PFN_vkEnumeratePhysicalDevices)get(context.instance,"vkEnumeratePhysicalDevices");
    PFN_vkGetPhysicalDeviceProperties2 properties=(PFN_vkGetPhysicalDeviceProperties2)get(context.instance,"vkGetPhysicalDeviceProperties2");
    PFN_vkGetPhysicalDeviceQueueFamilyProperties queues=(PFN_vkGetPhysicalDeviceQueueFamilyProperties)get(context.instance,"vkGetPhysicalDeviceQueueFamilyProperties");
    PFN_vkCreateDevice create_device=(PFN_vkCreateDevice)get(context.instance,"vkCreateDevice");
    PFN_vkGetDeviceProcAddr device_proc=(PFN_vkGetDeviceProcAddr)get(context.instance,"vkGetDeviceProcAddr");
    PFN_vkGetPhysicalDeviceExternalBufferProperties external=(PFN_vkGetPhysicalDeviceExternalBufferProperties)get(context.instance,"vkGetPhysicalDeviceExternalBufferProperties");
    context.memory_properties=(PFN_vkGetPhysicalDeviceMemoryProperties)get(context.instance,"vkGetPhysicalDeviceMemoryProperties");
    if(!context.destroy_instance||!enumerate||!properties||!queues||!create_device||!device_proc||!external||!context.memory_properties)goto fail;
    uint32_t count=16;VkPhysicalDevice devices[16];
    if(enumerate(context.instance,&count,devices)!=VK_SUCCESS || count>16)goto fail;
    for(uint32_t i=0;i<count;i++){
        VkPhysicalDeviceIDProperties id={.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
        VkPhysicalDeviceProperties2 p={.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2,.pNext=&id};
        properties(devices[i],&p);
        if(p.properties.vendorID==0x10de && memcmp(id.deviceUUID,uuid,16)==0 && memcmp(id.driverUUID,driver,16)==0){context.physical=devices[i];break;}
    }
    if(!context.physical)goto unsupported;
    VkBufferUsageFlags usage=VK_BUFFER_USAGE_TRANSFER_SRC_BIT|VK_BUFFER_USAGE_TRANSFER_DST_BIT|VK_BUFFER_USAGE_STORAGE_BUFFER_BIT;
    for(unsigned i=0;i<2;i++){
        VkPhysicalDeviceExternalBufferInfo info={.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_BUFFER_INFO,.usage=usage,.handleType=i?VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT:VK_EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_FD_BIT};
        VkExternalBufferProperties p={.sType=VK_STRUCTURE_TYPE_EXTERNAL_BUFFER_PROPERTIES};
        external(context.physical,&info,&p);
        if(!(p.externalMemoryProperties.externalMemoryFeatures&VK_EXTERNAL_MEMORY_FEATURE_EXPORTABLE_BIT))goto unsupported;
    }
    uint32_t qc=32;VkQueueFamilyProperties qp[32];queues(context.physical,&qc,qp);
    if(qc>32)goto fail;
    uint32_t qi=0;while(qi<qc && (!qp[qi].queueCount || !(qp[qi].queueFlags & VK_QUEUE_GRAPHICS_BIT)))qi++;
    if(qi==qc)goto unsupported;
    float priority=1.0f;
    VkDeviceQueueCreateInfo queue={.sType=VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,.queueFamilyIndex=qi,.queueCount=1,.pQueuePriorities=&priority};
    PFN_vkGetPhysicalDeviceExternalSemaphoreProperties semaphore_properties=(PFN_vkGetPhysicalDeviceExternalSemaphoreProperties)get(context.instance,"vkGetPhysicalDeviceExternalSemaphoreProperties");
    if(!semaphore_properties)goto unsupported;
    VkPhysicalDeviceExternalSemaphoreInfo si={.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_SEMAPHORE_INFO,.handleType=VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_OPAQUE_FD_BIT};
    VkExternalSemaphoreProperties sp={.sType=VK_STRUCTURE_TYPE_EXTERNAL_SEMAPHORE_PROPERTIES};
    semaphore_properties(context.physical,&si,&sp);
    if(!(sp.externalSemaphoreFeatures&VK_EXTERNAL_SEMAPHORE_FEATURE_EXPORTABLE_BIT))goto unsupported;
    const char *extensions[]={"VK_KHR_external_memory_fd","VK_EXT_external_memory_dma_buf","VK_KHR_external_semaphore_fd"};
    VkDeviceCreateInfo di={.sType=VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,.queueCreateInfoCount=1,.pQueueCreateInfos=&queue,.enabledExtensionCount=3,.ppEnabledExtensionNames=extensions};
    VkResult created=create_device(context.physical,&di,NULL,&context.device);
    if(created==VK_ERROR_EXTENSION_NOT_PRESENT)goto unsupported;
    if(created!=VK_SUCCESS)goto fail;
#define LOAD(field,type,name) context.field=(type)device_proc(context.device,name);if(!context.field)goto fail
    LOAD(destroy_device,PFN_vkDestroyDevice,"vkDestroyDevice");
    LOAD(create_buffer,PFN_vkCreateBuffer,"vkCreateBuffer");
    LOAD(destroy_buffer,PFN_vkDestroyBuffer,"vkDestroyBuffer");
    LOAD(requirements,PFN_vkGetBufferMemoryRequirements2,"vkGetBufferMemoryRequirements2");
    LOAD(allocate,PFN_vkAllocateMemory,"vkAllocateMemory");
    LOAD(free_memory,PFN_vkFreeMemory,"vkFreeMemory");
    LOAD(bind,PFN_vkBindBufferMemory,"vkBindBufferMemory");
    LOAD(get_fd,PFN_vkGetMemoryFdKHR,"vkGetMemoryFdKHR");
    LOAD(get_queue,PFN_vkGetDeviceQueue,"vkGetDeviceQueue");
    LOAD(create_pool,PFN_vkCreateCommandPool,"vkCreateCommandPool");
    LOAD(destroy_pool,PFN_vkDestroyCommandPool,"vkDestroyCommandPool");
    LOAD(allocate_commands,PFN_vkAllocateCommandBuffers,"vkAllocateCommandBuffers");
    LOAD(free_commands,PFN_vkFreeCommandBuffers,"vkFreeCommandBuffers");
    LOAD(begin,PFN_vkBeginCommandBuffer,"vkBeginCommandBuffer");
    LOAD(end,PFN_vkEndCommandBuffer,"vkEndCommandBuffer");
    LOAD(copy,PFN_vkCmdCopyBuffer,"vkCmdCopyBuffer");
    LOAD(barrier,PFN_vkCmdPipelineBarrier,"vkCmdPipelineBarrier");
    LOAD(create_fence,PFN_vkCreateFence,"vkCreateFence");
    LOAD(destroy_fence,PFN_vkDestroyFence,"vkDestroyFence");
    LOAD(reset_fences,PFN_vkResetFences,"vkResetFences");
    LOAD(fence_status,PFN_vkGetFenceStatus,"vkGetFenceStatus");
    LOAD(submit,PFN_vkQueueSubmit,"vkQueueSubmit");
    LOAD(create_semaphore,PFN_vkCreateSemaphore,"vkCreateSemaphore");
    LOAD(destroy_semaphore,PFN_vkDestroySemaphore,"vkDestroySemaphore");
    LOAD(semaphore_fd,PFN_vkGetSemaphoreFdKHR,"vkGetSemaphoreFdKHR");
#undef LOAD
    context.queue_family=qi;
    context.get_queue(context.device,qi,0,&context.queue);
    VkCommandPoolCreateInfo pool={.sType=VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,.queueFamilyIndex=qi};
    if(context.create_pool(context.device,&pool,NULL,&context.pool)!=VK_SUCCESS)goto fail;
    memcpy(context.uuid,uuid,16);memcpy(context.driver,driver,16);return 1;
unsupported:
    close_context();return -1;
fail:
    close_context();return 0;
}

/* Each export type uses a separate allocation: NVIDIA does not advertise
 * their joint compatibility. A bounded Vulkan copy bridges them on the GPU. */
static int make_buffer(VkExternalMemoryHandleTypeFlagBits handle, uint64_t bytes,
                       VkBuffer *buffer,VkDeviceMemory *memory,uint64_t *size,int *dedicated) {
    VkBufferUsageFlags usage=VK_BUFFER_USAGE_TRANSFER_SRC_BIT|VK_BUFFER_USAGE_TRANSFER_DST_BIT|VK_BUFFER_USAGE_STORAGE_BUFFER_BIT;
    /* Export support was checked when opening the matched device. */
    VkExternalMemoryBufferCreateInfo external={.sType=VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_BUFFER_CREATE_INFO,.handleTypes=handle};
    VkBufferCreateInfo bi={.sType=VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,.pNext=&external,.size=bytes,.usage=usage,.sharingMode=VK_SHARING_MODE_EXCLUSIVE};
    if(context.create_buffer(context.device,&bi,NULL,buffer)!=VK_SUCCESS)return 0;
    VkMemoryDedicatedRequirements dr={.sType=VK_STRUCTURE_TYPE_MEMORY_DEDICATED_REQUIREMENTS};
    VkMemoryRequirements2 req={.sType=VK_STRUCTURE_TYPE_MEMORY_REQUIREMENTS_2,.pNext=&dr};
    VkBufferMemoryRequirementsInfo2 ri={.sType=VK_STRUCTURE_TYPE_BUFFER_MEMORY_REQUIREMENTS_INFO_2,.buffer=*buffer};
    context.requirements(context.device,&ri,&req);
    if(req.memoryRequirements.size<bytes || req.memoryRequirements.size>64u*1024u*1024u)return 0;
    VkPhysicalDeviceMemoryProperties mp;context.memory_properties(context.physical,&mp);
    uint32_t type=mp.memoryTypeCount;
    for(uint32_t i=0;i<mp.memoryTypeCount && i<32;i++)if((req.memoryRequirements.memoryTypeBits&(1u<<i)) && (mp.memoryTypes[i].propertyFlags&VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT)){type=i;break;}
    if(type==mp.memoryTypeCount)return 0;
    /* Dedicated allocations also honor external-handle DEDICATED_ONLY. */
    *dedicated=1;
    VkMemoryDedicatedAllocateInfo da={.sType=VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO,.buffer=*buffer};
    VkExportMemoryAllocateInfo ex={.sType=VK_STRUCTURE_TYPE_EXPORT_MEMORY_ALLOCATE_INFO,.pNext=&da,.handleTypes=handle};
    VkMemoryAllocateInfo ai={.sType=VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,.pNext=&ex,.allocationSize=req.memoryRequirements.size,.memoryTypeIndex=type};
    if(context.allocate(context.device,&ai,NULL,memory)!=VK_SUCCESS || context.bind(context.device,*buffer,*memory,0)!=VK_SUCCESS)return 0;
    *size=req.memoryRequirements.size;return 1;
}

static void free_owner(Allocation *owner) {
    if(owner->initial_command)context.free_commands(context.device,context.pool,1,&owner->initial_command);
    if(owner->gl_ready)context.destroy_semaphore(context.device,owner->gl_ready,NULL);
    if(owner->vk_ready)context.destroy_semaphore(context.device,owner->vk_ready,NULL);
    if(owner->command)context.free_commands(context.device,context.pool,1,&owner->command);
    if(owner->fence)context.destroy_fence(context.device,owner->fence,NULL);
    if(owner->buffer)context.destroy_buffer(context.device,owner->buffer,NULL);
    if(owner->dma_buffer)context.destroy_buffer(context.device,owner->dma_buffer,NULL);
    if(owner->memory)context.free_memory(context.device,owner->memory,NULL);
    if(owner->dma_memory)context.free_memory(context.device,owner->dma_memory,NULL);
    free(owner);
}

/* No waits on the presentation thread, including context destruction. Retired
 * submissions retain their backing until a later zero-timeout poll completes.
 * The owner cap bounds even repeated context loss and an unresponsive device. */
static void collect_retired(void) {
    Allocation **cursor=&retired;
    while(*cursor){
        Allocation *owner=*cursor;
        if(owner->completion_unproven && context.fence_status(context.device,owner->fence)!=VK_SUCCESS){cursor=&owner->next;continue;}
        *cursor=owner->next;free_owner(owner);context.owners--;
    }
}

void *redunar_nvidia_memory_allocate(const uint8_t *uuid,const uint8_t *driver,uint64_t bytes,int *opaque_fd,int *dma_fd,uint64_t *allocation_bytes,int *dedicated,int *failure) {
    if(failure)*failure=0;
    if(!failure||!uuid||!driver||!opaque_fd||!dma_fd||!allocation_bytes||!dedicated||!bytes||bytes>64u*1024u*1024u)return NULL;
    *opaque_fd=-1;*dma_fd=-1;*allocation_bytes=0;*dedicated=0;
    pthread_mutex_lock(&mutex);
    Allocation *owner=NULL;
    int opened=open_context(uuid,driver);
    if(opened<=0){*failure=opened;goto fail;}
    collect_retired();
    if(context.owners>=64)goto fail;
    owner=calloc(1,sizeof(*owner));if(!owner)goto fail;
    uint64_t dma_bytes=0;int dma_dedicated=0;
    if(!make_buffer(VK_EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_FD_BIT,bytes,&owner->buffer,&owner->memory,allocation_bytes,dedicated))goto fail;
    if(!make_buffer(VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,bytes,&owner->dma_buffer,&owner->dma_memory,&dma_bytes,&dma_dedicated))goto fail;
    VkMemoryGetFdInfoKHR fi={.sType=VK_STRUCTURE_TYPE_MEMORY_GET_FD_INFO_KHR,.memory=owner->memory,.handleType=VK_EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_FD_BIT};
    if(context.get_fd(context.device,&fi,opaque_fd)!=VK_SUCCESS)goto fail;
    fi.memory=owner->dma_memory;fi.handleType=VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT;
    if(context.get_fd(context.device,&fi,dma_fd)!=VK_SUCCESS)goto fail;
    VkFenceCreateInfo fence={.sType=VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
    if(context.create_fence(context.device,&fence,NULL,&owner->fence)!=VK_SUCCESS)goto fail;
    VkExportSemaphoreCreateInfo export_sem={.sType=VK_STRUCTURE_TYPE_EXPORT_SEMAPHORE_CREATE_INFO,.handleTypes=VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_OPAQUE_FD_BIT};
    VkSemaphoreCreateInfo sci={.sType=VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO,.pNext=&export_sem};
    if(context.create_semaphore(context.device,&sci,NULL,&owner->gl_ready)!=VK_SUCCESS || context.create_semaphore(context.device,&sci,NULL,&owner->vk_ready)!=VK_SUCCESS)goto fail;
    VkCommandBufferAllocateInfo ca={.sType=VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,.commandPool=context.pool,.level=VK_COMMAND_BUFFER_LEVEL_PRIMARY,.commandBufferCount=1};
    if(context.allocate_commands(context.device,&ca,&owner->command)!=VK_SUCCESS)goto fail;
    VkCommandBufferBeginInfo begin={.sType=VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};
    if(context.begin(owner->command,&begin)!=VK_SUCCESS)goto fail;
    VkBufferMemoryBarrier acquire[2]={
        {.sType=VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER,.srcAccessMask=0,.dstAccessMask=VK_ACCESS_TRANSFER_READ_BIT,.srcQueueFamilyIndex=VK_QUEUE_FAMILY_EXTERNAL,.dstQueueFamilyIndex=context.queue_family,.buffer=owner->buffer,.size=VK_WHOLE_SIZE},
        {.sType=VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER,.srcAccessMask=0,.dstAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT,.srcQueueFamilyIndex=VK_QUEUE_FAMILY_EXTERNAL,.dstQueueFamilyIndex=context.queue_family,.buffer=owner->dma_buffer,.size=VK_WHOLE_SIZE}
    };
    context.barrier(owner->command,VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,VK_PIPELINE_STAGE_TRANSFER_BIT,0,0,NULL,2,acquire,0,NULL);
    VkBufferCopy region={.size=bytes};
    context.copy(owner->command,owner->buffer,owner->dma_buffer,1,&region);
    VkBufferMemoryBarrier release[2]={
        {.sType=VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER,.srcAccessMask=VK_ACCESS_TRANSFER_READ_BIT,.dstAccessMask=0,.srcQueueFamilyIndex=context.queue_family,.dstQueueFamilyIndex=VK_QUEUE_FAMILY_EXTERNAL,.buffer=owner->buffer,.size=VK_WHOLE_SIZE},
        {.sType=VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER,.srcAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT,.dstAccessMask=0,.srcQueueFamilyIndex=context.queue_family,.dstQueueFamilyIndex=VK_QUEUE_FAMILY_EXTERNAL,.buffer=owner->dma_buffer,.size=VK_WHOLE_SIZE}
    };
    context.barrier(owner->command,VK_PIPELINE_STAGE_TRANSFER_BIT,VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,0,0,NULL,2,release,0,NULL);
    if(context.end(owner->command)!=VK_SUCCESS)goto fail;
    /* Establish initial ownership release before GL touches either buffer. */
    if(context.allocate_commands(context.device,&ca,&owner->initial_command)!=VK_SUCCESS)goto fail;
    if(context.begin(owner->initial_command,&begin)!=VK_SUCCESS)goto fail;
    VkBufferMemoryBarrier initial_release[2]={release[0],release[1]};
    initial_release[0].srcAccessMask=0;initial_release[1].srcAccessMask=0;
    context.barrier(owner->initial_command,VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,0,0,NULL,2,initial_release,0,NULL);
    if(context.end(owner->initial_command)!=VK_SUCCESS)goto fail;
    VkSubmitInfo init={.sType=VK_STRUCTURE_TYPE_SUBMIT_INFO,.commandBufferCount=1,.pCommandBuffers=&owner->initial_command,.signalSemaphoreCount=1,.pSignalSemaphores=&owner->vk_ready};
    owner->completion_unproven=1;
    context.owners++;
    if(context.submit(context.queue,1,&init,owner->fence)!=VK_SUCCESS){
        owner->state=FAILED;owner->next=retired;retired=owner;owner=NULL;goto fail;
    }
    pthread_mutex_unlock(&mutex);return owner;
fail:
    if(*opaque_fd>=0){close(*opaque_fd);*opaque_fd=-1;}
    if(*dma_fd>=0){close(*dma_fd);*dma_fd=-1;}
    if(owner)free_owner(owner);
    if(!context.owners)close_context();
    pthread_mutex_unlock(&mutex);return NULL;
}

/* GL consumes successful imports. Close only FDs not yet passed to GL. */
int redunar_nvidia_memory_semaphores(void *allocation,int *gl_fd,int *vk_fd) {
    if(!allocation || !gl_fd || !vk_fd)return 0;
    *gl_fd=-1;*vk_fd=-1;
    pthread_mutex_lock(&mutex);
    Allocation *owner=allocation;
    VkSemaphoreGetFdInfoKHR info={.sType=VK_STRUCTURE_TYPE_SEMAPHORE_GET_FD_INFO_KHR,.semaphore=owner->gl_ready,.handleType=VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_OPAQUE_FD_BIT};
    int ok=context.semaphore_fd(context.device,&info,gl_fd)==VK_SUCCESS;
    info.semaphore=owner->vk_ready;
    if(ok)ok=context.semaphore_fd(context.device,&info,vk_fd)==VK_SUCCESS;
    if(!ok){if(*gl_fd>=0)close(*gl_fd);if(*vk_fd>=0)close(*vk_fd);*gl_fd=-1;*vk_fd=-1;}
    pthread_mutex_unlock(&mutex);return ok;
}

static int inject_once(const char *stage) {
    static unsigned fired;
    const char *setting=getenv("REDUNAR_REPLAY_GL_FAIL");
    unsigned bit=!strcmp(stage,"bridge-submit")?1u:2u;
    if(!setting || strcmp(setting,stage) || (fired&bit))return 0;
    fired|=bit;fprintf(stderr,"Redunar OpenGL Replay injected failure: %s\n",stage);return 1;
}

/* -1 failed, 0 pending, 1 ready. GL's fence proves its signal was submitted;
 * the semaphore establishes external memory availability and visibility. */
int redunar_nvidia_memory_poll(void *allocation) {
    if(!allocation || pthread_mutex_trylock(&mutex))return 0;
    Allocation *owner=allocation;
    collect_retired();
    if(owner->state==GL_OWNED){
        VkPipelineStageFlags stage=VK_PIPELINE_STAGE_TRANSFER_BIT;
        VkSubmitInfo submit={.sType=VK_STRUCTURE_TYPE_SUBMIT_INFO,.waitSemaphoreCount=1,.pWaitSemaphores=&owner->gl_ready,.pWaitDstStageMask=&stage,.commandBufferCount=1,.pCommandBuffers=&owner->command,.signalSemaphoreCount=1,.pSignalSemaphores=&owner->vk_ready};
        owner->completion_unproven=1;
        VkResult result=context.submit(context.queue,1,&submit,owner->fence);
        owner->state=(result==VK_SUCCESS && !inject_once("bridge-submit"))?COPY_PENDING:FAILED;
    }
    if(owner->state==COPY_PENDING){
        VkResult result=context.fence_status(context.device,owner->fence);
        if(inject_once("bridge-fence"))result=VK_ERROR_DEVICE_LOST;
        if(result==VK_SUCCESS){owner->state=READY;owner->completion_unproven=0;}
        else if(result!=VK_NOT_READY)owner->state=FAILED;
    }
    int result=owner->state==FAILED?-1:owner->state==READY;
    pthread_mutex_unlock(&mutex);return result;
}

/* GL only waits on a completed release. Every poll has zero timeout. */
int redunar_nvidia_memory_rearm(void *allocation) {
    if(!allocation || pthread_mutex_trylock(&mutex))return 0;
    Allocation *owner=allocation;
    if(owner->state==INITIALIZING){
        VkResult status=context.fence_status(context.device,owner->fence);
        if(status==VK_SUCCESS){owner->state=READY;owner->completion_unproven=0;}
        else if(status!=VK_NOT_READY)owner->state=FAILED;
    }
    int result=0;
    if(owner->state==READY){
        if(context.reset_fences(context.device,1,&owner->fence)==VK_SUCCESS){owner->state=GL_OWNED;result=1;}
        else owner->state=FAILED;
    }else if(owner->state==GL_OWNED)result=1;
    if(owner->state==FAILED)result=-1;
    pthread_mutex_unlock(&mutex);return result;
}

void redunar_nvidia_memory_free(void *allocation) {
    if(!allocation)return;
    pthread_mutex_lock(&mutex);
    Allocation *owner=allocation;
    collect_retired();
    if(owner->completion_unproven && context.fence_status(context.device,owner->fence)!=VK_SUCCESS){
        owner->next=retired;retired=owner;
    }else{
        free_owner(owner);
        if(--context.owners==0)close_context();
    }
    pthread_mutex_unlock(&mutex);
}
