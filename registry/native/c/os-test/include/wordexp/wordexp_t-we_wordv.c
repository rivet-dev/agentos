#include <wordexp.h>
void foo(wordexp_t* bar)
{
	char ***qux = &bar->we_wordv;
	(void) qux;
}
int main(void) { return 0; }
