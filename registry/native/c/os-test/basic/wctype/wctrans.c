/* Test whether a basic wctrans invocation works. */

#include <wctype.h>

#include "../basic.h"

int main(void)
{
	wctrans_t desc = wctrans("tolower");
	if ( !desc )
		err(1, "wctrans");
	return 0;
}
