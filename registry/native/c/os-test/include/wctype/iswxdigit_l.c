#include <wctype.h>
#ifdef iswxdigit_l
#undef iswxdigit_l
#endif
int (*foo)(wint_t, locale_t) = iswxdigit_l;
int main(void) { return 0; }
