/* Test %g with a precision, which should format like a %f with a precision of 2. */

#include "suite.h"

int main(void)
{
	printf("'%.4g'\n", 42.690000);
	return 0;
}
