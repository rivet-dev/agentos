/* Test whether a basic bsearch invocation works. */

#include <stdlib.h>

#include "../basic.h"

int compare_int(const void* a, const void* b)
{
	if ( *((int*) a) < *((int*) b) )
		return -1;
	if ( *((int*) a) > *((int*) b) )
		return 1;
	return 0;
}

int main(void)
{
	int numbers[] = { 6, 9, 13, 42, 101, 1337, 9001 };
	int seek = 101;
	void* ptr = bsearch(&seek, &numbers, sizeof(numbers) / sizeof(int),
	                    sizeof(int), compare_int);
	if ( !ptr )
		errx(1, "bsearch did not find %d", seek);
	if ( *((int*) ptr) != seek )
		errx(1, "bsearch found %d instead of %d", *((int*) ptr), seek);
	return 0;
}
