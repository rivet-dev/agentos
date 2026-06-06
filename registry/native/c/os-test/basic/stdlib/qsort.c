/* Test whether a basic qsort invocation works. */

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
	int numbers[] = { 6, 101, 9001, 13, 1337, 42, 9, };
	qsort(numbers, sizeof(numbers) / sizeof(int), sizeof(int), compare_int);
	for ( size_t i = 1; i < sizeof(numbers) / sizeof(int); i++ )
		if ( numbers[i - 1] > numbers[i] )
			errx(1, "out of order: %d > %d", numbers[i - 1], numbers[i]);
	return 0;
}
