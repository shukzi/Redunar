use crate::ffi::{
    PfnCreateSwapchainKhr, PfnDestroyDevice, PfnDestroyInstance, PfnDestroySwapchainKhr,
    PfnGetDeviceProcAddr, PfnGetDeviceQueue, PfnGetDeviceQueue2, PfnGetInstanceProcAddr,
    PfnQueuePresentKhr,
};
use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex, MutexGuard};

#[derive(Clone, Copy)]
pub(crate) struct InstanceDispatch {
    // Raw dispatchable handles are not `Send`; retain the address and restore
    // the typed handle only at the Vulkan call boundary.
    pub(crate) instance_address: usize,
    pub(crate) next_get_instance_proc_addr: PfnGetInstanceProcAddr,
    pub(crate) destroy_instance: Option<PfnDestroyInstance>,
}

#[derive(Clone, Copy)]
pub(crate) struct DeviceDispatch {
    pub(crate) next_get_device_proc_addr: PfnGetDeviceProcAddr,
    pub(crate) destroy_device: Option<PfnDestroyDevice>,
    pub(crate) get_device_queue: Option<PfnGetDeviceQueue>,
    pub(crate) get_device_queue2: Option<PfnGetDeviceQueue2>,
    pub(crate) create_swapchain: Option<PfnCreateSwapchainKhr>,
    pub(crate) destroy_swapchain: Option<PfnDestroySwapchainKhr>,
    pub(crate) queue_present: Option<PfnQueuePresentKhr>,
}

static INSTANCES: LazyLock<Mutex<BTreeMap<usize, InstanceDispatch>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
static DEVICES: LazyLock<Mutex<BTreeMap<usize, DeviceDispatch>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
static GLOBAL_NEXT_INSTANCE: LazyLock<Mutex<Option<PfnGetInstanceProcAddr>>> =
    LazyLock::new(|| Mutex::new(None));

pub(crate) fn insert_instance(key: usize, dispatch: InstanceDispatch) {
    lock(&INSTANCES).insert(key, dispatch);
}

pub(crate) fn instance(key: usize) -> Option<InstanceDispatch> {
    lock(&INSTANCES).get(&key).copied()
}

pub(crate) fn remove_instance(key: usize) -> Option<InstanceDispatch> {
    lock(&INSTANCES).remove(&key)
}

pub(crate) fn insert_device(key: usize, dispatch: DeviceDispatch) {
    lock(&DEVICES).insert(key, dispatch);
}

pub(crate) fn device(key: usize) -> Option<DeviceDispatch> {
    lock(&DEVICES).get(&key).copied()
}

pub(crate) fn remove_device(key: usize) -> Option<DeviceDispatch> {
    lock(&DEVICES).remove(&key)
}

pub(crate) fn set_global_next_instance(function: PfnGetInstanceProcAddr) {
    *lock(&GLOBAL_NEXT_INSTANCE) = Some(function);
}

pub(crate) fn global_next_instance() -> Option<PfnGetInstanceProcAddr> {
    *lock(&GLOBAL_NEXT_INSTANCE)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
