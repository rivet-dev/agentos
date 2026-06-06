#include <wchar.h>
#ifdef fputwc
#undef fputwc
#endif
wint_t (*foo)(wchar_t, FILE *) = fputwc;
int main(void) { return 0; }
