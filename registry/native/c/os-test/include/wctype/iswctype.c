#include <wctype.h>
#ifdef iswctype
#undef iswctype
#endif
int (*foo)(wint_t, wctype_t) = iswctype;
int main(void) { return 0; }
