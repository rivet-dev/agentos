/* Tests whether realloc(ptr, 0) returns non-zero. */

#include "malloc.h"

int main(void)
{
	void* ptr = malloc(1);
	if ( !ptr )
		err(1, "malloc");
	errno = 0;
	void* newptr = realloc(ptr, 0);
	if ( newptr )
		puts("non-NULL");
	else if ( 0 < errno )
		err(1, "realloc");
	else
	{
		/* realloc returns NULL without setting errno. That means the allocation
		   been freed and we didn't get a replacement allocation. This behavior
		   is undesirable in my opinion because it causes much more compexity
		   and makes realloc much harder to use without a check for whether size
		   is zero. Unfortunately C11 with DR400
		   <http://open-std.org/jtc1/sc22/wg14/www/docs/summary.htm#dr_400>
		   now allows this behavior and marks using realloc with size == 0 as
		   obsolescent. POSIX issue 7 (2018) also allows this behavior, even
		   though its rationale doesn't like it. It does require errno to be set
		   to an implementation specific value in this case, which no
		   implementation does, so I guess that means keeping errno unchanged.
		   Therefore this case is allowed by the standards. It does make the
		   interface much harder to use for arbitrary lengths and can cause
		   double free and use after free bugs if software doesn't know to take
		   care. */
		puts("NULL");
	}
	return 0;
}
