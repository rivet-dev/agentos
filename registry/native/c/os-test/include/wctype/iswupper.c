#include <wctype.h>
#ifdef iswupper
#undef iswupper
#endif
int (*foo)(wint_t) = iswupper;
int main(void) { return 0; }
