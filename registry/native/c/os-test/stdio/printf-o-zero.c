/* Test printing zero with zero precision with %o */

#include "suite.h"

int main(void)
{
	printf("'%.0u'\n", 0);
	return 0;
}
