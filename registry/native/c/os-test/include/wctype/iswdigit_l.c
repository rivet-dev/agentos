#include <wctype.h>
#ifdef iswdigit_l
#undef iswdigit_l
#endif
int (*foo)(wint_t, locale_t) = iswdigit_l;
int main(void) { return 0; }
