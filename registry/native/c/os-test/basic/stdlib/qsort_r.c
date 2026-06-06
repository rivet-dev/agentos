/* Test whether a basic qsort_r invocation works. */

#include <stdlib.h>

#include "../basic.h"

int compare_int_r(const void* a, const void* b, void* ctx)
{
	int mult = *((int*) ctx);
	if ( *((int*) a) * mult < *((int*) b) * mult )
		return -1;
	if ( *((int*) a) * mult > *((int*) b) * mult )
		return 1;
	return 0;
}

int main(void)
{
	int mult = -1;
	int numbers[] = { 6, 101, 9001, 13, 1337, 42, 9, };
	qsort_r(numbers, sizeof(numbers) / sizeof(int), sizeof(int), compare_int_r,
	        &mult);
	for ( size_t i = 1; i < sizeof(numbers) / sizeof(int); i++ )
		if ( numbers[i - 1] < numbers[i] )
			errx(1, "out of order: %d < %d", numbers[i - 1], numbers[i]);
	return 0;
}
