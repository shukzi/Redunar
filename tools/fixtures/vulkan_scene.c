/* Independently implemented bounded presentation fixture. No game code.
 * Vulkan 1.1 is explicit: distro vkcube variants can request only 1.0, which
 * Redunar intentionally leaves metrics-only. Each image contains a changing
 * color, sufficient to exercise export, encoding and decoding without shaders.
 */
#define _POSIX_C_SOURCE 200809L
#define VK_USE_PLATFORM_XCB_KHR
#include <vulkan/vulkan.h>
#include <xcb/xcb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define CHECK(call) do { VkResult result = (call); if (result != VK_SUCCESS) { \
    fprintf(stderr, "%s failed: %d\n", #call, result); goto cleanup; } } while (0)
#define CHECK_WSI(call) do { VkResult result = (call); \
    if (result != VK_SUCCESS && result != VK_SUBOPTIMAL_KHR) { \
        fprintf(stderr, "%s failed: %d\n", #call, result); goto cleanup; } } while (0)

int main(int argc, char **argv) {
    unsigned width = 640, height = 240, frames = 1200;
    if (argc != 4) {
        fprintf(stderr, "usage: vulkan-scene WIDTH HEIGHT FRAMES\n");
        return 2;
    }
    unsigned *values[] = {&width, &height, &frames};
    for (int i = 1; i < argc; ++i) {
        char *end = NULL;
        unsigned long value = strtoul(argv[i], &end, 10);
        if (!*argv[i] || *end || value == 0 || value > 4000) return 2;
        *values[i - 1] = (unsigned)value;
    }
    fprintf(stderr, "Generated Vulkan 1.1 scene: %ux%u, %u frames\n", width, height, frames);

    int status = 1, screen_number = 0;
    xcb_connection_t *connection = xcb_connect(NULL, &screen_number);
    VkInstance instance = VK_NULL_HANDLE;
    VkSurfaceKHR surface = VK_NULL_HANDLE;
    VkDevice device = VK_NULL_HANDLE;
    VkSwapchainKHR swapchain = VK_NULL_HANDLE;
    VkCommandPool pool = VK_NULL_HANDLE;
    VkSemaphore acquired = VK_NULL_HANDLE, rendered = VK_NULL_HANDLE;
    xcb_window_t window = XCB_WINDOW_NONE;
    if (xcb_connection_has_error(connection)) {
        fprintf(stderr, "XCB display connection failed\n"); goto cleanup;
    }
    xcb_screen_iterator_t screens = xcb_setup_roots_iterator(xcb_get_setup(connection));
    for (int i = 0; i < screen_number; ++i) xcb_screen_next(&screens);
    xcb_screen_t *screen = screens.data;
    window = xcb_generate_id(connection);
    xcb_create_window(connection, XCB_COPY_FROM_PARENT, window, screen->root,
        0, 0, (uint16_t)width, (uint16_t)height, 0, XCB_WINDOW_CLASS_INPUT_OUTPUT,
        screen->root_visual, 0, NULL);
    const char title[] = "Redunar isolated Vulkan Replay check";
    xcb_change_property(connection, XCB_PROP_MODE_REPLACE, window, XCB_ATOM_WM_NAME,
        XCB_ATOM_STRING, 8, sizeof(title) - 1, title);
    /* Fixed bounds keep a tiling compositor from expanding the tiny probe into
     * an unintended full-display encoding workload. */
    uint32_t size_hints[18] = {0};
    size_hints[0] = (1u << 4) | (1u << 5);
    size_hints[5] = size_hints[7] = width;
    size_hints[6] = size_hints[8] = height;
    xcb_change_property(connection, XCB_PROP_MODE_REPLACE, window, XCB_ATOM_WM_NORMAL_HINTS,
        XCB_ATOM_WM_SIZE_HINTS, 32, 18, size_hints);
    xcb_map_window(connection, window);
    xcb_flush(connection);

    VkApplicationInfo application = {.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
        .pApplicationName = "Redunar generated scene", .apiVersion = VK_API_VERSION_1_1};
    const char *instance_extensions[] = {VK_KHR_SURFACE_EXTENSION_NAME, VK_KHR_XCB_SURFACE_EXTENSION_NAME};
    VkInstanceCreateInfo create_instance = {.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        .pApplicationInfo = &application, .enabledExtensionCount = 2,
        .ppEnabledExtensionNames = instance_extensions};
    CHECK(vkCreateInstance(&create_instance, NULL, &instance));
    VkXcbSurfaceCreateInfoKHR create_surface = {.sType = VK_STRUCTURE_TYPE_XCB_SURFACE_CREATE_INFO_KHR,
        .connection = connection, .window = window};
    CHECK(vkCreateXcbSurfaceKHR(instance, &create_surface, NULL, &surface));

    VkPhysicalDevice physical = VK_NULL_HANDLE, devices[32];
    uint32_t device_count = 32, queue_family = 0;
    VkResult enumerated = vkEnumeratePhysicalDevices(instance, &device_count, devices);
    if (enumerated != VK_SUCCESS && enumerated != VK_INCOMPLETE) goto cleanup;
    for (uint32_t d = 0; d < device_count && physical == VK_NULL_HANDLE; ++d) {
        VkPhysicalDeviceProperties properties;
        vkGetPhysicalDeviceProperties(devices[d], &properties);
        if (properties.apiVersion < VK_API_VERSION_1_1) continue;
        VkQueueFamilyProperties queues[64];
        uint32_t queue_count = 64;
        vkGetPhysicalDeviceQueueFamilyProperties(devices[d], &queue_count, queues);
        for (uint32_t q = 0; q < queue_count; ++q) {
            VkBool32 supported = VK_FALSE;
            CHECK(vkGetPhysicalDeviceSurfaceSupportKHR(devices[d], q, surface, &supported));
            if (supported && (queues[q].queueFlags & VK_QUEUE_GRAPHICS_BIT)) {
                physical = devices[d]; queue_family = q; break;
            }
        }
    }
    if (physical == VK_NULL_HANDLE) {
        fprintf(stderr, "No Vulkan 1.1 graphics/presentation device\n"); goto cleanup;
    }
    float priority = 1.0f;
    VkDeviceQueueCreateInfo create_queue = {.sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
        .queueFamilyIndex = queue_family, .queueCount = 1, .pQueuePriorities = &priority};
    const char *device_extensions[] = {VK_KHR_SWAPCHAIN_EXTENSION_NAME, VK_KHR_EXTERNAL_MEMORY_FD_EXTENSION_NAME};
    VkDeviceCreateInfo create_device = {.sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
        .queueCreateInfoCount = 1, .pQueueCreateInfos = &create_queue,
        .enabledExtensionCount = 2, .ppEnabledExtensionNames = device_extensions};
    CHECK(vkCreateDevice(physical, &create_device, NULL, &device));
    VkQueue queue;
    vkGetDeviceQueue(device, queue_family, 0, &queue);

    VkSurfaceCapabilitiesKHR capabilities;
    CHECK(vkGetPhysicalDeviceSurfaceCapabilitiesKHR(physical, surface, &capabilities));
    VkSurfaceFormatKHR formats[64], format = {0};
    uint32_t format_count = 64;
    enumerated = vkGetPhysicalDeviceSurfaceFormatsKHR(physical, surface, &format_count, formats);
    if (enumerated != VK_SUCCESS && enumerated != VK_INCOMPLETE) goto cleanup;
    for (uint32_t i = 0; i < format_count; ++i) {
        if (formats[i].format == VK_FORMAT_B8G8R8A8_UNORM || formats[i].format == VK_FORMAT_R8G8B8A8_UNORM
            || formats[i].format == VK_FORMAT_B8G8R8A8_SRGB || formats[i].format == VK_FORMAT_R8G8B8A8_SRGB) {
            format = formats[i]; break;
        }
    }
    if (!format.format || !(capabilities.supportedUsageFlags & VK_IMAGE_USAGE_TRANSFER_DST_BIT)
        || !(capabilities.supportedUsageFlags & VK_IMAGE_USAGE_TRANSFER_SRC_BIT)) {
        fprintf(stderr, "No supported 8-bit transfer swapchain\n"); goto cleanup;
    }
    VkExtent2D extent = capabilities.currentExtent;
    if (extent.width == UINT32_MAX) extent = (VkExtent2D){width, height};
    if (extent.width != width || extent.height != height) {
        fprintf(stderr, "Surface size %ux%u differs from requested %ux%u\n",
            extent.width, extent.height, width, height); goto cleanup;
    }
    uint32_t count = capabilities.minImageCount + 1;
    if (capabilities.maxImageCount && count > capabilities.maxImageCount) count = capabilities.maxImageCount;
    VkCompositeAlphaFlagBitsKHR alpha = VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR;
    if (!(capabilities.supportedCompositeAlpha & alpha)) {
        alpha = (VkCompositeAlphaFlagBitsKHR)(capabilities.supportedCompositeAlpha &
            (~capabilities.supportedCompositeAlpha + 1u));
    }
    VkSwapchainCreateInfoKHR create_swapchain = {.sType = VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR,
        .surface = surface, .minImageCount = count, .imageFormat = format.format,
        .imageColorSpace = format.colorSpace, .imageExtent = extent, .imageArrayLayers = 1,
        .imageUsage = VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT,
        .imageSharingMode = VK_SHARING_MODE_EXCLUSIVE, .preTransform = capabilities.currentTransform,
        .compositeAlpha = alpha, .presentMode = VK_PRESENT_MODE_FIFO_KHR, .clipped = VK_TRUE};
    CHECK(vkCreateSwapchainKHR(device, &create_swapchain, NULL, &swapchain));
    VkImage images[32];
    uint32_t image_count = 32;
    CHECK(vkGetSwapchainImagesKHR(device, swapchain, &image_count, images));
    VkCommandPoolCreateInfo create_pool = {.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
        .flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT, .queueFamilyIndex = queue_family};
    CHECK(vkCreateCommandPool(device, &create_pool, NULL, &pool));
    VkCommandBufferAllocateInfo allocate = {.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        .commandPool = pool, .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY, .commandBufferCount = 1};
    VkCommandBuffer command;
    CHECK(vkAllocateCommandBuffers(device, &allocate, &command));
    VkSemaphoreCreateInfo create_semaphore = {.sType = VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO};
    CHECK(vkCreateSemaphore(device, &create_semaphore, NULL, &acquired));
    CHECK(vkCreateSemaphore(device, &create_semaphore, NULL, &rendered));

    for (unsigned frame = 0; frame < frames; ++frame) {
        uint32_t image;
        CHECK_WSI(vkAcquireNextImageKHR(device, swapchain, 2000000000ULL, acquired, VK_NULL_HANDLE, &image));
        CHECK(vkResetCommandBuffer(command, 0));
        VkCommandBufferBeginInfo begin = {.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
            .flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT};
        CHECK(vkBeginCommandBuffer(command, &begin));
        VkImageSubresourceRange range = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
        VkImageMemoryBarrier barrier = {.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
            .dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT, .oldLayout = VK_IMAGE_LAYOUT_UNDEFINED,
            .newLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
            .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED, .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
            .image = images[image], .subresourceRange = range};
        vkCmdPipelineBarrier(command, VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT,
            0, 0, NULL, 0, NULL, 1, &barrier);
        VkClearColorValue color = {.float32 = {(float)(frame % 120) / 120.0f, 0.15f, 0.4f, 1.0f}};
        vkCmdClearColorImage(command, images[image], VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &color, 1, &range);
        barrier.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT; barrier.dstAccessMask = 0;
        barrier.oldLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL; barrier.newLayout = VK_IMAGE_LAYOUT_PRESENT_SRC_KHR;
        vkCmdPipelineBarrier(command, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,
            0, 0, NULL, 0, NULL, 1, &barrier);
        CHECK(vkEndCommandBuffer(command));
        VkPipelineStageFlags wait_stage = VK_PIPELINE_STAGE_TRANSFER_BIT;
        VkSubmitInfo submit = {.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO, .waitSemaphoreCount = 1,
            .pWaitSemaphores = &acquired, .pWaitDstStageMask = &wait_stage,
            .commandBufferCount = 1, .pCommandBuffers = &command,
            .signalSemaphoreCount = 1, .pSignalSemaphores = &rendered};
        CHECK(vkQueueSubmit(queue, 1, &submit, VK_NULL_HANDLE));
        VkPresentInfoKHR present = {.sType = VK_STRUCTURE_TYPE_PRESENT_INFO_KHR, .waitSemaphoreCount = 1,
            .pWaitSemaphores = &rendered, .swapchainCount = 1, .pSwapchains = &swapchain, .pImageIndices = &image};
        CHECK_WSI(vkQueuePresentKHR(queue, &present));
        /* Test-only serialization makes semaphore reuse independent of WSI
         * scheduling. This fixture does not measure game performance. */
        CHECK(vkQueueWaitIdle(queue));
        struct timespec delay = {0, 16666667};
        nanosleep(&delay, NULL);
    }
    status = 0;
cleanup:
    if (device) {
        vkDeviceWaitIdle(device);
        if (rendered) vkDestroySemaphore(device, rendered, NULL);
        if (acquired) vkDestroySemaphore(device, acquired, NULL);
        if (pool) vkDestroyCommandPool(device, pool, NULL);
        if (swapchain) vkDestroySwapchainKHR(device, swapchain, NULL);
        vkDestroyDevice(device, NULL);
    }
    if (surface) vkDestroySurfaceKHR(instance, surface, NULL);
    if (instance) vkDestroyInstance(instance, NULL);
    if (window) xcb_destroy_window(connection, window);
    xcb_disconnect(connection);
    fprintf(stderr, "Generated Vulkan scene exit=%d\n", status);
    return status;
}
