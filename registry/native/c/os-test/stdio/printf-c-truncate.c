/* Test argument truncation of %c, where the int argument shall be converted to an unsigned char */

#include "suite.h"

int main(void)
{
	printf("'%c'\n", 0x4141);
	return 0;
}
