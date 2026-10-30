/* Window ownership for the bounded Vulkan fixture. Default XCB is unchanged;
 * the optional installed SDL2 route explicitly requires native Wayland WSI.
 * SDL entry points follow the public SDL2 API; no SDK implementation is copied. */
#include <dlfcn.h>
typedef struct {
    xcb_connection_t *xcb;
    xcb_window_t xid;
    void *library, *sdl;
    void (*quit)(void);
    void (*destroy)(void *);
    int (*extensions)(void *,unsigned *,const char **);
    int (*surface)(void *,VkInstance,VkSurfaceKHR *);
    void (*events)(void);
} SceneWindow;
static int scene_window_open(SceneWindow *w,unsigned width,unsigned height) {
    const char *wsi=getenv("REDUNAR_VULKAN_WSI");
    if(wsi && !strcmp(wsi,"wayland")) {
        if(setenv("SDL_VIDEODRIVER","wayland",1))return 0;
        w->library=dlopen("libSDL2-2.0.so.0",RTLD_NOW|RTLD_LOCAL);
        if(!w->library)return 0;
        int (*init)(uint32_t)=(int (*)(uint32_t))dlsym(w->library,"SDL_Init");
        void *(*create)(const char *,int,int,int,int,uint32_t)=(void *(*)(const char *,int,int,int,int,uint32_t))dlsym(w->library,"SDL_CreateWindow");
        const char *(*driver)(void)=(const char *(*)(void))dlsym(w->library,"SDL_GetCurrentVideoDriver");
        w->quit=(void (*)(void))dlsym(w->library,"SDL_Quit");
        w->destroy=(void (*)(void *))dlsym(w->library,"SDL_DestroyWindow");
        w->extensions=(int (*)(void *,unsigned *,const char **))dlsym(w->library,"SDL_Vulkan_GetInstanceExtensions");
        w->surface=(int (*)(void *,VkInstance,VkSurfaceKHR *))dlsym(w->library,"SDL_Vulkan_CreateSurface");
        w->events=(void (*)(void))dlsym(w->library,"SDL_PumpEvents");
        if(!init||!create||!driver||!w->quit||!w->destroy||!w->extensions||!w->surface||!w->events)return 0;
        if(init(0x20u) || !driver() || strcmp(driver(),"wayland"))return 0;
        w->sdl=create("Redunar native Wayland Vulkan check",0x1fff0000,0x1fff0000,(int)width,(int)height,0x10000004u);
        fprintf(stderr,"Fixture WSI=wayland (SDL driver=%s), requested=%ux%u\n",driver(),width,height);
        return w->sdl!=NULL;
    }
    int screen_number=0;
    w->xcb=xcb_connect(NULL,&screen_number);
    if(xcb_connection_has_error(w->xcb))return 0;
    xcb_screen_iterator_t screens=xcb_setup_roots_iterator(xcb_get_setup(w->xcb));
    for(int i=0;i<screen_number;i++)xcb_screen_next(&screens);
    xcb_screen_t *screen=screens.data;
    w->xid=xcb_generate_id(w->xcb);
    xcb_create_window(w->xcb,XCB_COPY_FROM_PARENT,w->xid,screen->root,0,0,(uint16_t)width,(uint16_t)height,0,XCB_WINDOW_CLASS_INPUT_OUTPUT,screen->root_visual,0,NULL);
    const char title[]="Redunar isolated Vulkan Replay check";
    xcb_change_property(w->xcb,XCB_PROP_MODE_REPLACE,w->xid,XCB_ATOM_WM_NAME,XCB_ATOM_STRING,8,sizeof(title)-1,title);
    uint32_t hints[18]={0};hints[0]=(1u<<4)|(1u<<5);hints[5]=hints[7]=width;hints[6]=hints[8]=height;
    xcb_change_property(w->xcb,XCB_PROP_MODE_REPLACE,w->xid,XCB_ATOM_WM_NORMAL_HINTS,XCB_ATOM_WM_SIZE_HINTS,32,18,hints);
    xcb_map_window(w->xcb,w->xid);xcb_flush(w->xcb);
    fprintf(stderr,"Fixture WSI=xcb\n");return 1;
}
static int scene_window_extensions(SceneWindow *w,unsigned *count,const char **names) {
    if(w->sdl)return w->extensions(w->sdl,count,names);
    if(*count<2)return 0;
    *count=2;names[0]=VK_KHR_SURFACE_EXTENSION_NAME;names[1]=VK_KHR_XCB_SURFACE_EXTENSION_NAME;return 1;
}
static int scene_window_surface(SceneWindow *w,VkInstance instance,VkSurfaceKHR *surface) {
    if(w->sdl)return w->surface(w->sdl,instance,surface);
    VkXcbSurfaceCreateInfoKHR ci={.sType=VK_STRUCTURE_TYPE_XCB_SURFACE_CREATE_INFO_KHR,.connection=w->xcb,.window=w->xid};
    return vkCreateXcbSurfaceKHR(instance,&ci,NULL,surface)==VK_SUCCESS;
}
static void scene_window_events(SceneWindow *w) {if(w->sdl)w->events();}
static void scene_window_close(SceneWindow *w) {
    if(w->sdl)w->destroy(w->sdl);
    if(w->quit)w->quit();
    if(w->library)dlclose(w->library);
    if(w->xid)xcb_destroy_window(w->xcb,w->xid);
    if(w->xcb)xcb_disconnect(w->xcb);
}
