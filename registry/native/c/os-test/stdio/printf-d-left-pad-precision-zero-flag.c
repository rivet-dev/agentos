/* Test correct width padding with a specified precision for %d */

#include "suite.h"

int main(void)
{
	printf("'%04.2d'\n", 7);
	return 0;
}
