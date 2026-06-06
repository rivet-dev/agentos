#include <wchar.h>
#ifdef wcsxfrm
#undef wcsxfrm
#endif
size_t (*foo)(wchar_t *restrict, const wchar_t *restrict, size_t) = wcsxfrm;
int main(void) { return 0; }
