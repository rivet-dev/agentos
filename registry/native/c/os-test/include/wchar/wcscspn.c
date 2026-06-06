#include <wchar.h>
#ifdef wcscspn
#undef wcscspn
#endif
size_t (*foo)(const wchar_t *, const wchar_t *) = wcscspn;
int main(void) { return 0; }
