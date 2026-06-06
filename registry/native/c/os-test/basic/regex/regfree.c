/* Test whether a basic regfree invocation works. */

#include <regex.h>

#include "../basic.h"

int main(void)
{
	int ret;
	regex_t re;
	const char* regex = "^foo*([oO]oba{3,4}r)$";
	if ( (ret = regcomp(&re, regex, REG_EXTENDED)) )
	{
		char msg[256];
		regerror(ret, &re, msg, sizeof(msg));
		errx(1, "regcomp: %s", msg);
	}
	regfree(&re);
	return 0;
}
