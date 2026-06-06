/* Test whether a basic reallocarray invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	int* numbers = calloc(4, sizeof(int));
	if ( !numbers )
		err(1, "malloc");
	numbers[0] = 0;
	numbers[1] = 1;
	for ( size_t i = 2; i < 4; i++ )
		numbers[i] = numbers[i-2] + numbers[i-1];
	numbers = reallocarray(numbers, 16, sizeof(int));
	if ( !numbers )
		err(1, "reallocarray");
	for ( size_t i = 4; i < 16; i++ )
		numbers[i] = numbers[i-2] + numbers[i-1];
	if ( numbers[15] != 610 )
		err(1, "incorrect: got %d wanted %d", numbers[15], 610);
	return 0;
}
