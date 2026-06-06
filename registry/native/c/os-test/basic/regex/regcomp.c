/* Test whether a basic regcomp invocation works. */

#include <regex.h>

#include "../basic.h"

int main(void)
{
	int ret;
	regex_t bre;
	const char* re1 = "^?+foo*ba{1,3}r.*(\\(\\)\\[|\\|[^]a-z][[]$";
	if ( (ret = regcomp(&bre, re1, 0)) )
	{
		char msg[256];
		regerror(ret, &bre, msg, sizeof(msg));
		errx(1, "regcomp: %s", msg);
	}
	regex_t ere;
	const char* re2 = "^\\?\\+f?o+o*ba{1,3}r.*\\(()\\[|\\|[^]a-z][[]$";
	if ( (ret = regcomp(&ere, re2, REG_EXTENDED)) )
	{
		char msg[256];
		regerror(ret, &ere, msg, sizeof(msg));
		errx(1, "regcomp: %s", msg);
	}
	return 0;
}
