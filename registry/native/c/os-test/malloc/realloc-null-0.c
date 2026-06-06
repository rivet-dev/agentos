/* Tests whether realloc(NULL, 0) returns non-zero. */

#include "malloc.h"

int main(void)
{
	errno = 0;
	void* newptr = realloc(NULL, 0);
	if ( newptr )
		puts("non-NULL");
	else if ( errno )
		err(1, "realloc");
	else
		puts("NULL");
	return 0;
}
