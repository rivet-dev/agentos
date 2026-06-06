/* Test support for supplying values to conversion with positional arguments */

#include "suite.h"

int main(void)
{
	printf("'%3$c%1$c%4$c%4$c%2$c'\n", 'e', 'o', 'h', 'l');
	return 0;
}
