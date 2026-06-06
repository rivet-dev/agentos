/* Tests whether malloc(0) returns non-zero. */

#include "malloc.h"

int main(void)
{
	errno = 0;
	void* ptr = malloc(0);
	if ( ptr )
		puts("non-NULL");
	else if ( errno != 0 )
		err(1, "malloc");
	else
		puts("NULL");
	return 0;
}
