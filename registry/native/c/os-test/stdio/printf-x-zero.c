/* Test printing zero with zero precision with %x */

#include "suite.h"

int main(void)
{
	printf("'%.0x'\n", 0);
	return 0;
}
