#include <wctype.h>
#ifdef iswspace_l
#undef iswspace_l
#endif
int (*foo)(wint_t, locale_t) = iswspace_l;
int main(void) { return 0; }
