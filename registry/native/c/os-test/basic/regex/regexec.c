/* Test whether a basic regexec invocation works. */

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
	const char* string = "foooooooobaaaar";
	regmatch_t matches[2];
	if ( (ret = regexec(&re, string, 2, matches, 0)) )
	{
		char msg[256];
		regerror(ret, &re, msg, sizeof(msg));
		errx(1, "regcomp: %s", msg);
	}
	if ( (size_t) matches[0].rm_so != 0 &&
	     (size_t) matches[0].rm_eo != strlen(string) )
		errx(1, "regex did not match entire string");
	if ( (size_t) matches[1].rm_so != 7 &&
	     (size_t) matches[1].rm_eo != strlen(string) - 7 )
		errx(1, "subexpr matched incorrectly");
	return 0;
}
