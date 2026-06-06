/* Test %g with the '#' flag, which should NOT trim trailing zeroes. */

#include "suite.h"

int main(void)
{
	printf("'%#g'\n", 42.690000);
	return 0;
}
