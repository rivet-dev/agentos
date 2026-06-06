#include <regex.h>
void foo(regmatch_t* bar)
{
	regoff_t *qux = &bar->rm_so;
	(void) qux;
}
int main(void) { return 0; }
