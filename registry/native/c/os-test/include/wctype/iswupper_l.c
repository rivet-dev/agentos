#include <wctype.h>
#ifdef iswupper_l
#undef iswupper_l
#endif
int (*foo)(wint_t, locale_t) = iswupper_l;
int main(void) { return 0; }
