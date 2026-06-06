/* Test %Lf formatting with positional arguments. */

#include "suite.h"

int main(void)
{
	printf("'%2$0*1$.*3$Lf'\n", 9, 1234.56789L, 3);
	return 0;
}
