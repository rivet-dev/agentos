/* Test padding of INFINITY with %f */

#include "suite.h"

int main(void)
{
	printf("'%09f'\n", INFINITY);
	return 0;
}
