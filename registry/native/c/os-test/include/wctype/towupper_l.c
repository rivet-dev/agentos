#include <wctype.h>
#ifdef towupper_l
#undef towupper_l
#endif
wint_t (*foo)(wint_t, locale_t) = towupper_l;
int main(void) { return 0; }
