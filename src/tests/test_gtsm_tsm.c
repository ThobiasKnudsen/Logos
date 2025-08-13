#include "tklog.h"
#include "gtsm.h"

struct node_1 {
	struct tsm_base_node base;
	float x;
	float y;
	float width;
	float height;
	unsigned char red;
	unsigned char green;
	unsigned char blue;
	unsigned char alpha;
};

static bool node_1_free(struct tsm_base_node* p_base) {
	if (!tsm_base_node_free(p_base)) {
		tklog_error("failed to free node_1\n");
		return false;
	}
	return true;
}
static void node_1_free_callback(struct rcu_head* p_rcu_head) {
	struct tsm_base_node* p_base = caa_container_of(p_rcu_head, struct tsm_base_node, rcu_head);
	if (!node_1_free(p_base)) {
		tklog_error("node_1_free failed\n");
	}
}
static bool node_1_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
	if (!tsm_base_node_is_valid(p_tsm_base, p_base)) {
		return false;
	}
	struct node_1* p_node_1 = caa_container_of(p_base, struct node_1, base);
	if (p_node_1->x-p_node_1->width < 0) {
		return false;
	}
	if (p_node_1->y-p_node_1->height < 0) {
		return false;
	}
	return true;
}
static bool node_1_print_info(struct tsm_base_node* p_base) {
	if (!tsm_base_node_print_info(p_base)) {
		return false;
	}
	struct node_1* p_node_1 = caa_container_of(p_base, struct node_1, base);
	tklog_info("p_node_1 %p: ", p_node_1);
	tklog_info("    x y widht height: %f %f %f %f\n", p_node_1->x, p_node_1->y, p_node_1->width, p_node_1->height);
	tklog_info("    red green blue alpha: %d %d %d %d\n", p_node_1->red, p_node_1->green, p_node_1->blue, p_node_1->alpha);
	return true;
}

bool test_function() {
	tklog_info("hello\n");
	tklog_scope(tklog_error("hello\n"););
	return false;
}

int main() {

	tklog_info("test_gtsm_tsm running\n");

	rcu_init();
	rcu_register_thread();

	tklog_scope(bool init_result = gtsm_init());
	if (!init_result) {
		tklog_error("gtsm_init failed\n");
		return -1;
	}

	tklog_scope(struct tsm_key_ctx node_1_type_key = tsm_key_ctx_create(0, "node_1_type", false));

	tklog_scope(struct tsm_base_node* node_1_type_base= tsm_base_type_node_create(
		node_1_type_key,
		sizeof(struct tsm_base_type_node),
		node_1_free,
		node_1_free_callback,
		node_1_is_valid,
		node_1_print_info,
		sizeof(struct node_1)));

	rcu_read_lock();
	tklog_scope(bool insert_result = gtsm_node_insert(node_1_type_base));
	if (!insert_result) {
		tklog_error("inserting node failed\n");
	}
	rcu_read_unlock();

	

	synchronize_rcu();
	gtsm_clean();
	rcu_unregister_thread();

	return 0;
}