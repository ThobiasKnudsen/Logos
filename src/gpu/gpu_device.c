#include "gpu/gpu_device.h"

bool gpu_device_tsm_create(struct tsm_key key_tsm) {

	struct tsm_key gpu_device_type_key = tsm_key_create(0, "gpu_device_type", false);
	struct tsm_base_node* p_new_type_node = tsm_base_type_node_create(
												gpu_device_type_key
												sizeof(struct tsm_base_type_node),
												_gpu_device_type_free_callback,
												_gpu_device_type_is_valid,
												_gpu_device_type_print,
												sizeof(struct gpu_device));
	if (!p_new_tsm) {
		tsm_key_free(gpu_device_type_key);
		tklog_error("tsm_base_type_node_create failed\n");
		return false;
	}

	struct tsm_key gpu_devices_key = tsm_key_create(0, "gpu_devices", false);
	struct tsm_base_node* p_new_tsm = tsm_create(gtsm_get(), gpu_devices_key);
	if (!p_new_tsm) {
		tsm_key_free(gpu_devices_key);
		tsm_base_node_free(p_new_type_node);
		tklog_error("tsm_create failed\n");
		return false;
	}


}
bool gpu_device_tsm_is_created();
struct tsm_base_node* gpu_device_tsm_get();
struct tsm_path gpu_device_tsm_get_path();
bool gpu_device_tsm_delete();

bool gpu_device_create(struct tsm_key key, );
struct tsm_base_node* gpu_device_get(struct tsm_key key);