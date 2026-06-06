/* Test truncation to short with %d */

#include "suite.h"

int main(void)
{
	printf("'%hd'\n", 123456);
	return 0;
}
