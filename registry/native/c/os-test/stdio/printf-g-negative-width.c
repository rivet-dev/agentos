/* Test formatting %g with negative width, which should be taken as a '-' flag
 * followed by a positive width. */

#include "suite.h"

int main(void)
{
	printf("'%*g'\n", -5, 15.1);
	return 0;
}
