/* Test alternative form of %e force-emitting the decimal point. */

#include "suite.h"

int main(void)
{
	printf("'%#.0e'\n", -42.0);
	return 0;
}
