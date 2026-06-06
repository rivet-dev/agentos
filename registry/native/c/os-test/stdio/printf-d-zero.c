/* Test printing zero with zero precision with %d */

#include "suite.h"

int main(void)
{
	printf("'%.0d'\n", 0);
	return 0;
}
