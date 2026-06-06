/* Test whether a basic aligned_alloc invocation works. */

#include <stdint.h>
#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	void* ptr = aligned_alloc(256, 768);
	if ( !ptr )
		err(1, "aligned_alloc");
	uintptr_t intptr = (uintptr_t) ptr;
	if ( intptr & 0xFF )
		errx(1, "aligned_alloc did not 256-byte align");
	return 0;
}
