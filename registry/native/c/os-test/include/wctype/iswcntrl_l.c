#include <wctype.h>
#ifdef iswcntrl_l
#undef iswcntrl_l
#endif
int (*foo)(wint_t, locale_t) = iswcntrl_l;
int main(void) { return 0; }
