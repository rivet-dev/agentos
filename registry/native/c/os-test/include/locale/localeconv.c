#include <locale.h>
#ifdef localeconv
#undef localeconv
#endif
struct lconv *(*foo)(void) = localeconv;
int main(void) { return 0; }
