/* Test whether a basic atomic_thread_fence invocation works. */

#include <stdatomic.h>

#include "../basic.h"

int main(void)
{
	// There is no defined way to observe the fence, so just do it once.
	atomic_thread_fence(memory_order_seq_cst);
	return 0;
}
