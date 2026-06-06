#include <wctype.h>
#ifdef iswdigit
#undef iswdigit
#endif
int (*foo)(wint_t) = iswdigit;
int main(void) { return 0; }
