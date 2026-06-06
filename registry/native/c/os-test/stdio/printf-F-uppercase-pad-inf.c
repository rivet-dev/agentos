/* Test padding of INFINITY with %F */

#include "suite.h"

int main(void)
{
	printf("'%09F'\n", INFINITY);
	return 0;
}
