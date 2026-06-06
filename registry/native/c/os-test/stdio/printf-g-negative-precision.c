/* Test formatting %g with a negative precision, which should be treated
 * as if the precision was omitted. */

#include "suite.h"

int main(void)
{
	printf("'%.*g'\n", -2, 15.1234567);
	return 0;
}
